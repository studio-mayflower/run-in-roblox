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
/// Studio is normally spawned directly and owned as a child process, which is
/// the only form that guarantees it can be killed. A background launch has to
/// go through macOS' `open`, which exits as soon as the app is launched and
/// leaves us with nothing but whatever pids we can find ourselves.
// Only a macOS build takes the `open` path, so only it constructs the other two.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
enum StudioProcess {
    Owned(process::Child),
    /// Every process holding this run's place open. More than one is normal:
    /// Studio runs helpers under the same name.
    Pids(Vec<u32>),
    /// Studio was launched, but we could not work out which process it is, so
    /// there is nothing to kill.
    Unknown,
}

impl Drop for StudioProcess {
    fn drop(&mut self) {
        match self {
            StudioProcess::Owned(child) => {
                let _ignored = child.kill();
            }
            StudioProcess::Pids(pids) => {
                // SIGKILL, not SIGTERM: Studio does not quit on a polite signal
                // when it was launched this way.
                let mut kill = Command::new("kill");
                kill.arg("-9");
                for pid in pids.iter() {
                    kill.arg(pid.to_string());
                }
                let _ignored = kill.stdout(Stdio::null()).stderr(Stdio::null()).status();
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

    /// Launch Studio without letting it take over the screen.
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

    /// Studio ignores LaunchServices' "launch hidden" flag (`open -j`), so a
    /// hidden run is two steps: `open -g` starts it without activating it, and
    /// then System Events hides it the way Command-H does. The first use of
    /// that prompts once for permission to control System Events (Privacy &
    /// Security -> Automation); decline it and the run still happens, with a
    /// window.
    ///
    /// `open` exits as soon as the app is launched and never reports a pid, so
    /// the processes to hide and later kill are found by their argv: the place
    /// is a file in a temp directory unique to this run, so anything holding it
    /// open is ours, and a Studio the user already had open never matches. `-n`
    /// makes it a new instance rather than a document opened in theirs.
    ///
    /// The place is passed with `--args` so Studio sees the same argv it does
    /// on a direct spawn.
    #[cfg(target_os = "macos")]
    fn start_studio_hidden(
        &self,
        studio_install: &RobloxStudio,
    ) -> Result<StudioProcess, anyhow::Error> {
        use std::ffi::OsStr;

        let application_path = studio_install.application_path();

        // .../RobloxStudio.app/Contents/MacOS/RobloxStudio -> .../RobloxStudio.app
        let app_bundle = application_path
            .ancestors()
            .find(|path| path.extension() == Some(OsStr::new("app")))
            .ok_or_else(|| anyhow!("Roblox Studio's executable was not inside a .app bundle"))?
            .to_owned();

        let place_arg = format!("{}", self.place_path.display());

        let status = Command::new("open")
            .arg("-n")
            .arg("-g")
            .arg("-a")
            .arg(&app_bundle)
            .arg("--args")
            .arg(&place_arg)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("Could not run `open` to launch Roblox Studio")?;

        if !status.success() {
            bail!("`open` failed to launch Roblox Studio ({})", status);
        }

        let launched = find_launched_studio(&place_arg);

        if let StudioProcess::Pids(pids) = &launched {
            hide(pids);
        }

        Ok(launched)
    }

    #[cfg(not(target_os = "macos"))]
    fn start_studio_hidden(
        &self,
        _studio_install: &RobloxStudio,
    ) -> Result<StudioProcess, anyhow::Error> {
        bail!("launching Roblox Studio hidden is only implemented on macOS")
    }
}

/// `open` reports no pid, so this run's Studio is whatever is holding this run's
/// place file open. Every match is ours -- the path is in a temp directory made
/// for this run -- so helper processes sharing the name are killed too rather
/// than left behind, which is what matching on the process name got wrong.
#[cfg(target_os = "macos")]
fn find_launched_studio(place_arg: &str) -> StudioProcess {
    use std::time::Instant;

    let deadline = Instant::now() + Duration::from_secs(30);

    loop {
        match pids_holding(place_arg) {
            Ok(pids) if !pids.is_empty() => return StudioProcess::Pids(pids),
            Ok(_) => {}
            Err(err) => {
                log::warn!(
                    "Could not list processes, so the Studio this run launched will not be \
                     closed for you. Quit it by hand. ({:#})",
                    err
                );
                return StudioProcess::Unknown;
            }
        }

        if Instant::now() >= deadline {
            log::error!(
                "Roblox Studio never appeared as a process holding {}, so it will not be closed \
                 for you and will keep running after this command exits. Quit it by hand, and \
                 consider --show-window.",
                place_arg
            );
            return StudioProcess::Unknown;
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(target_os = "macos")]
fn pids_holding(place_arg: &str) -> Result<Vec<u32>, anyhow::Error> {
    let output = Command::new("pgrep")
        .arg("-f")
        .arg(place_arg)
        .output()
        .context("Could not run `pgrep`")?;

    // pgrep exits with 1 when nothing matched, which is not an error here.
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect())
}

/// Hide the launched app, retrying while it finishes coming up: System Events
/// has no process to hide until then. Only one of the pids is the app; the rest
/// are helpers with no window, and asking about those simply fails.
///
/// A run is not worth failing over a window, so this only warns.
#[cfg(target_os = "macos")]
fn hide(pids: &[u32]) {
    use std::time::Instant;

    let deadline = Instant::now() + Duration::from_secs(20);

    loop {
        for pid in pids {
            // A raw string: the script has its own quotes, and a Rust line
            // continuation would eat the space in front of `(first`.
            let script = format!(
                r#"tell application "System Events" to set visible of (first application process whose unix id is {}) to false"#,
                pid
            );
            let out = Command::new("osascript").arg("-e").arg(&script).output();
            if matches!(out, Ok(ref o) if o.status.success()) {
                return;
            }
        }

        if Instant::now() >= deadline {
            log::warn!(
                "Could not hide Roblox Studio, so this run has a window. macOS asks once for \
                 permission to control System Events (Privacy & Security -> Automation); \
                 without it there is no way to hide another app."
            );
            return;
        }

        std::thread::sleep(Duration::from_millis(250));
    }
}
