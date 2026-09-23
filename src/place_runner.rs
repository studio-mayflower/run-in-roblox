use std::{
    path::PathBuf,
    process::{self, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

// Only the macOS build hides Studio, and only it has a thread doing the hiding.
#[cfg(target_os = "macos")]
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
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

impl StudioProcess {
    /// Keep Studio hidden for as long as the returned handle is held.
    ///
    /// Only a hidden launch has pids to hide; a visible one is meant to be
    /// seen, and a launch we lost track of has nothing to act on.
    #[cfg(target_os = "macos")]
    fn keep_hidden(&self) -> Option<Hider> {
        match self {
            StudioProcess::Pids(pids) => Some(Hider::start(pids.clone())),
            _ => None,
        }
    }
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

        let studio_process = self.start_studio(&studio_install)?;

        // Held for the length of the run: one hide at launch does not stick.
        // Dropped before `studio_process` -- there is no point hiding a process
        // on its way to being killed.
        #[cfg(target_os = "macos")]
        let _hider = studio_process.keep_hidden();

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

        // Kill Studio first: nothing below needs it, and the place file it
        // holds open lives in a temp directory the caller deletes as soon as
        // this returns -- an orphan outliving that directory is what put a
        // modal "could not open the place" error on screen.
        drop(studio_process);

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

        Ok(find_launched_studio(&place_arg))
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

/// How often a hidden run asks for Studio to be hidden again while it is still
/// starting up, and how long that lasts.
///
/// Studio un-hides itself exactly once, when it presents the place window, and
/// the window is on screen until the next ask -- so this interval is how long
/// it flashes. Asking four times a second costs four `osascript` processes a
/// second, which is worth it while the window is expected and not for the rest
/// of a run that can last five minutes.
#[cfg(target_os = "macos")]
const HIDE_INTERVAL_STARTUP: Duration = Duration::from_millis(250);

#[cfg(target_os = "macos")]
const HIDE_STARTUP: Duration = Duration::from_secs(60);

/// How often to ask once the window has settled, purely as a backstop.
#[cfg(target_os = "macos")]
const HIDE_INTERVAL_SETTLED: Duration = Duration::from_secs(2);

/// How long hiding may fail before we call it refused permission and stop.
#[cfg(target_os = "macos")]
const HIDE_GIVE_UP: Duration = Duration::from_secs(20);

/// Keeps Studio hidden for the length of a run, and stops on drop.
///
/// Hiding once at launch is not enough. Studio un-hides itself when it presents
/// the place window, and it does that on its own schedule: after the launch
/// hide, and after the plugin has already reported in -- so there is no message
/// to hang a single second hide on either. Asking again on a timer is what
/// makes it stay down, and a hide that lands while the window is already up
/// does stick.
#[cfg(target_os = "macos")]
struct Hider {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

#[cfg(target_os = "macos")]
impl Hider {
    fn start(pids: Vec<u32>) -> Hider {
        use std::time::Instant;

        let stop = Arc::new(AtomicBool::new(false));

        let thread = {
            let stop = Arc::clone(&stop);

            thread::spawn(move || {
                let started = Instant::now();
                let mut ever_hid = false;

                while !stop.load(Ordering::Relaxed) {
                    if hide_once(&pids) {
                        ever_hid = true;
                    } else if !ever_hid && started.elapsed() >= HIDE_GIVE_UP {
                        // Nothing has worked for long enough that it is not a
                        // timing problem: the permission was refused.
                        log::warn!(
                            "Could not hide Roblox Studio, so this run has a window. macOS asks \
                             once for permission to control System Events (Privacy & Security -> \
                             Automation); without it there is no way to hide another app."
                        );
                        return;
                    }

                    let interval = if started.elapsed() < HIDE_STARTUP {
                        HIDE_INTERVAL_STARTUP
                    } else {
                        HIDE_INTERVAL_SETTLED
                    };

                    // Slept in slices so dropping the handle does not wait out
                    // a whole interval.
                    for _ in 0..10 {
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        thread::sleep(interval / 10);
                    }
                }
            })
        };

        Hider {
            stop,
            thread: Some(thread),
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for Hider {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);

        if let Some(thread) = self.thread.take() {
            let _ignored = thread.join();
        }
    }
}

/// Hide every pid of this run, the way Command-H does, and say whether anything
/// was hidden.
///
/// Every pid, not just up to the first that answers: a helper is an application
/// process too, so it answers as successfully as the app while having no window
/// to hide.
#[cfg(target_os = "macos")]
fn hide_once(pids: &[u32]) -> bool {
    let mut hid_any = false;

    for pid in pids {
        // A raw string: the script has its own quotes, and a Rust line
        // continuation would eat the space in front of `(first`.
        let script = format!(
            r#"tell application "System Events" to set visible of (first application process whose unix id is {}) to false"#,
            pid
        );
        let out = Command::new("osascript").arg("-e").arg(&script).output();
        if matches!(out, Ok(ref o) if o.status.success()) {
            hid_any = true;
        }
    }

    hid_any
}
