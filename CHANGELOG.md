# run-in-roblox Changelog

## Unreleased Changes

## 0.4.2 (2026-09-23)
0.4.1's hidden launch still showed a window, and still left Studio behind. Both
are fixed, this time against what Studio actually does.

* Keep Studio hidden, instead of hiding it once. Studio un-hides itself when it
  presents the place window, which happens after the launch-time hide -- and
  after the plugin has reported in, so there was no message to hang a second
  hide on either. A hidden run now re-hides on a timer for as long as it lasts
  (four times a second for the first minute, then every two seconds), which
  leaves the window on screen only for the moment between it being presented
  and the next ask.
* Do not exit before Studio has been killed. `PlaceRunner::run` owns the handle
  whose drop kills Studio, and it runs on its own thread, while `main` called
  `process::exit` as soon as the run reported its last message -- and
  `process::exit` does not run other threads' destructors, so the kill was a
  race the runner thread lost essentially every time. `main` now joins that
  thread before exiting, and the run kills Studio before it removes the plugin
  file rather than after.

  The leftover Studio also outlived the temp directory holding the place it was
  opening, which is why every run ended with a "We could not open the place
  ... Cannot open place file for reading" dialog to dismiss.
* A run that fails now reports the failure. The runner thread's error was
  unwrapped into a panic message and the main thread reported "receiving on a
  closed channel" instead; the error now comes back through the join.
* Hiding tries every pid of a run rather than stopping at the first that
  answers: Studio's helpers are application processes too, so one of them
  answering counted as a success while the app kept its window.

## 0.4.1 (2026-09-22)
0.4.0's hidden launch did not work, and left Studio behind. Both are fixed.

* Actually hide Studio. `open -j` asks LaunchServices to start an app hidden,
  and Studio ignores it, so 0.4.0 showed a window every run. The window is now
  hidden after launch, the way Command-H does, through System Events. macOS asks
  once for permission to control it (Privacy & Security -> Automation); decline
  and the run still happens, with a window and a warning.
* Stop leaking Studio processes. 0.4.0 found the process to kill by matching the
  process name and gave up unless exactly one new match appeared -- and Studio
  runs helpers under that name, so the common case was to kill nothing and leave
  a Studio (and its window) behind on every run. The processes are now found by
  their argv: the place lives in a temp directory unique to the run, so every
  match is this run's and all of them are killed, while a Studio the user
  already had open never matches.
* `open -g` still keeps the launch from stealing focus, and `--show-window`
  still opts out of all of it.

## 0.4.0 (2026-09-22)
* Launch Roblox Studio hidden on macOS, so a test run no longer takes over the
  screen or steals focus. Pass `--show-window` for the old behaviour.

  A window belongs to the app, so a process we spawn ourselves cannot be told
  to start without one -- only LaunchServices can. A hidden launch therefore
  goes through `open -n -g -j`, and the place is passed with `--args` so Studio
  sees the same argv it did before. `open` reports no pid, so the Studio to
  close at the end of the run is found by diffing the running Studio processes
  across the launch.

  Nothing about this works off macOS, and a hidden launch that fails for any
  reason falls back to a normal one: a run that takes over the screen is a much
  smaller problem than a run that does not happen.

## 0.3.1 (2026-08-26)
This is the first release of the studio-mayflower fork. The tool's behavior is
unchanged from upstream 0.3.0 -- no source file was touched. Only the way it is
built and published differs.

* Publish a native `aarch64-apple-darwin` binary. Upstream only ever published
  an x86_64 macOS build, so Apple Silicon machines ran it under Rosetta 2, which
  Apple reduces to a gaming-only subset in macOS 28.
* Name every asset with its architecture (`...-macos-aarch64.zip`,
  `...-macos-x86_64.zip`, ...) so Rokit resolves the native binary instead of
  falling back to an OS-only match.
* Build each target with an explicit `--target` rather than relying on the
  runner's host architecture.
* Replace the Node 12 based `actions/checkout@v1` and `upload-artifact@v1`,
  which no longer run on GitHub's current runner images, and have the release
  workflow attach assets to the GitHub Release instead of leaving them as
  workflow artifacts to be attached by hand.
* Cut releases from a merge to `master` rather than a hand-pushed tag, matching
  the `mf-storybook` / `rblx-studio-mcp` workflows: `version` in `Cargo.toml` is
  the source of truth, and a push whose version already has a release is a
  no-op. See "Releasing" in the README.

## 0.3.0 (2020-07-19)
* **Breaking**: Reworked command line interface from the ground-up.
	* Places are now passed with `--place`.
	* Scripts are now passed via `--script`. A script is now always required.
* Added support for any place file, not just ones that rbx-dom supports.
* Fixed many panics, replacing them with graceful error messages.

## 0.2.0
* **TODO**

## 0.1.0
* Initial release