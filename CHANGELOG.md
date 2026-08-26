# run-in-roblox Changelog

## Unreleased Changes

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