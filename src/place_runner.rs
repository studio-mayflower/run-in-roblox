use std::{
    path::PathBuf,
    process::{self, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use anyhow::{anyhow, bail, Context};
use fs_err as fs;
use fs_err::File;
use roblox_install::RobloxStudio;

use crate::{
    message_receiver::{Message, MessageReceiver, MessageReceiverOptions, RobloxMessage},
    plugin::RunInRbxPlugin,
};

/// A handle to the Roblox Studio we started, which force-kills it on drop.
///
/// Studio is normally spawned directly and owned as a child process. A hidden
/// launch has to go through macOS' `open`, which exits as soon as the app is
/// launched and leaves us with nothing but a pid.
// Only a macOS build takes the `open` path, so only it constructs the other two.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
enum StudioProcess {
    Owned(process::Child),
    Pid(u32),
    /// Studio was launched, but we could not work out which process it is.
    Unknown,
}

impl Drop for StudioProcess {
    fn drop(&mut self) {
        match self {
            StudioProcess::Owned(child) => {
                let _ignored = child.kill();
            }
            StudioProcess::Pid(pid) => {
                let _ignored = Command::new("kill")
                    .arg("-9")
                    .arg(pid.to_string())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            StudioProcess::Unknown => {}
        }
    }
}

pub struct PlaceRunner {
    pub port: u16,
    pub place_path: PathBuf,
    pub server_id: String,
    pub lua_script: String,

    /// Launch Studio without showing its window.
    pub hidden: bool,
}

impl PlaceRunner {
    pub fn run(&self, sender: mpsc::Sender<Option<RobloxMessage>>) -> Result<(), anyhow::Error> {
        let studio_install =
            RobloxStudio::locate().context("Could not locate a Roblox Studio installation.")?;

        let plugin_file_path = studio_install
            .plugins_path()
            .join(format!("run_in_roblox-{}.rbxmx", self.port));

        let plugin = RunInRbxPlugin {
            port: self.port,
            server_id: &self.server_id,
            lua_script: &self.lua_script,
        };

        let plugin_file = File::create(&plugin_file_path)?;
        plugin.write(plugin_file)?;

        let message_receiver = MessageReceiver::start(MessageReceiverOptions {
            port: self.port,
            server_id: self.server_id.to_owned(),
        });

        let _studio_process = self.start_studio(&studio_install)?;

        let first_message = message_receiver
            .recv_timeout(Duration::from_secs(60))
            .ok_or_else(|| {
                anyhow!("Timeout reached while waiting for Roblox Studio to come online")
            })?;

        match first_message {
            Message::Start => {}
            _ => bail!("Invalid first message received from Roblox Studio plugin"),
        }

        loop {
            match message_receiver.recv() {
                Message::Start => {}
                Message::Stop => {
                    sender.send(None)?;
                    break;
                }
                Message::Messages(roblox_messages) => {
                    for message in roblox_messages.into_iter() {
                        sender.send(Some(message))?;
                    }
                }
            }
        }

        message_receiver.stop();
        fs::remove_file(&plugin_file_path)?;

        Ok(())
    }

    /// A failed hidden launch falls back to a normal one: a run that takes over
    /// the screen is a far smaller problem than a run that does not happen.
    fn start_studio(&self, studio_install: &RobloxStudio) -> Result<StudioProcess, anyhow::Error> {
        if self.hidden {
            match self.start_studio_hidden(studio_install) {
                Ok(process) => return Ok(process),
                Err(err) => log::warn!(
                    "Could not launch Roblox Studio hidden, falling back to a normal launch: {:#}",
                    err
                ),
            }
        }

        self.start_studio_visible(studio_install)
    }

    fn start_studio_visible(
        &self,
        studio_install: &RobloxStudio,
    ) -> Result<StudioProcess, anyhow::Error> {
        Ok(StudioProcess::Owned(
            Command::new(studio_install.application_path())
                .arg(format!("{}", self.place_path.display()))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?,
        ))
    }

    /// There is no way to hide the window of a process we spawn ourselves: the
    /// window belongs to the app, and only LaunchServices can be asked to start
    /// one hidden. `open -j` asks for exactly that, and `-g` keeps the app from
    /// taking focus, so a hidden launch goes through `open` instead of exec'ing
    /// the binary.
    ///
    /// The place is passed with `--args` so that Studio sees the same argv it
    /// does today, rather than being handed the file as a document to open.
    #[cfg(target_os = "macos")]
    fn start_studio_hidden(
        &self,
        studio_install: &RobloxStudio,
    ) -> Result<StudioProcess, anyhow::Error> {
        use std::ffi::OsStr;

        let application_path = studio_install.application_path();

        let process_name = application_path
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or_else(|| anyhow!("Roblox Studio's path had no file name"))?
            .to_owned();

        // .../RobloxStudio.app/Contents/MacOS/RobloxStudio -> .../RobloxStudio.app
        let app_bundle = application_path
            .ancestors()
            .find(|path| path.extension() == Some(OsStr::new("app")))
            .ok_or_else(|| anyhow!("Roblox Studio's executable was not inside a .app bundle"))?
            .to_owned();

        let before = studio_pids(&process_name)?;

        let status = Command::new("open")
            // A new instance, so that the pid below cannot be a Studio the user
            // already had open, and so that an open Studio is left alone.
            .arg("-n")
            // Do not bring it to the foreground.
            .arg("-g")
            // Launch it hidden.
            .arg("-j")
            .arg("-a")
            .arg(&app_bundle)
            .arg("--args")
            .arg(format!("{}", self.place_path.display()))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("Could not run `open` to launch Roblox Studio")?;

        if !status.success() {
            bail!("`open` failed to launch Roblox Studio ({})", status);
        }

        Ok(find_launched_studio(&process_name, &before))
    }

    #[cfg(not(target_os = "macos"))]
    fn start_studio_hidden(
        &self,
        _studio_install: &RobloxStudio,
    ) -> Result<StudioProcess, anyhow::Error> {
        bail!("launching Roblox Studio hidden is only implemented on macOS")
    }
}

/// `open` reports no pid, so the Studio it launched is whichever Studio process
/// is there now and was not there before.
#[cfg(target_os = "macos")]
fn find_launched_studio(
    process_name: &str,
    before: &std::collections::HashSet<u32>,
) -> StudioProcess {
    use std::time::Instant;

    let deadline = Instant::now() + Duration::from_secs(30);

    loop {
        match studio_pids(process_name) {
            Ok(now) => {
                let launched: Vec<u32> = now.difference(before).copied().collect();

                match launched.as_slice() {
                    [pid] => return StudioProcess::Pid(*pid),
                    [] => {}
                    _ => {
                        // Another Studio started at the same moment as ours, and
                        // killing the wrong one would take down the user's editor.
                        log::warn!(
                            "More than one Roblox Studio started while this run was launching, \
                             so this run's Studio cannot be told apart from the others and will \
                             not be closed for you. Quit the hidden Studio by hand."
                        );
                        return StudioProcess::Unknown;
                    }
                }
            }
            Err(err) => {
                log::warn!(
                    "Could not list Roblox Studio processes, so the Studio this run launched \
                     will not be closed for you. Quit it by hand. ({:#})",
                    err
                );
                return StudioProcess::Unknown;
            }
        }

        if Instant::now() >= deadline {
            log::warn!(
                "Roblox Studio did not appear as a running process, so it will not be closed \
                 for you. If a hidden Studio is running, quit it by hand."
            );
            return StudioProcess::Unknown;
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(target_os = "macos")]
fn studio_pids(process_name: &str) -> Result<std::collections::HashSet<u32>, anyhow::Error> {
    let output = Command::new("pgrep")
        .arg("-x")
        .arg(process_name)
        .output()
        .context("Could not run `pgrep` to find Roblox Studio processes")?;

    // pgrep exits with 1 when nothing matched, which is not an error here.
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect())
}
