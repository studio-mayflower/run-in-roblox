# run-in-roblox Changelog

## Unreleased Changes

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