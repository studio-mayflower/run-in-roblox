# run-in-roblox
A studio-mayflower fork of [rojo-rbx/run-in-roblox](https://github.com/rojo-rbx/run-in-roblox), which has been unmaintained since 2020.

The source is unmodified. The fork exists to publish a **native Apple Silicon binary**: upstream's only macOS asset is x86_64, so it requires Rosetta 2 — which Apple cuts down to a gaming-only subset in macOS 28. See [CHANGELOG.md](CHANGELOG.md) for what changed.

run-in-roblox is a tool to run a place, a model, or an individual script inside Roblox Studio.

run-in-roblox pipes output from inside Roblox Studio back to stdout/stderr, which enables traditional automation tools to work alongside Roblox.

## Installation

### With [Rokit](https://github.com/rojo-rbx/rokit)
```toml
[tools]
run-in-roblox = "studio-mayflower/run-in-roblox@0.3.1"
```

Assets are named with their architecture, so Rokit installs the native binary on Apple Silicon rather than the x86_64 one.

### From GitHub Releases
You can download pre-built binaries from [the Releases page](https://github.com/studio-mayflower/run-in-roblox/releases).

### From source
```bash
cargo install --git https://github.com/studio-mayflower/run-in-roblox --locked
```

(`cargo install run-in-roblox` installs upstream 0.3.0 from crates.io, which is the x86_64-only build this fork exists to replace.)

## Releasing
Releases are cut from a merge to `master`, with no manual tagging (the
`mf-storybook` / `rblx-studio-mcp` pattern).

The version's single source of truth is `version` in `Cargo.toml`. On every push
to `master`, the workflow reads it and publishes `v<version>` unless that
release already exists, so merges that don't touch the version are no-ops.

To release, bump the version in a PR:

```bash
cargo set-version 0.3.2   # or edit Cargo.toml and run: cargo update -p run-in-roblox
```

Both `Cargo.toml` and `Cargo.lock` have to move together — the workflow fails
early if they disagree, because `cargo build --locked` would fail anyway.

## Usage
The recommended way to use `run-in-roblox` is with a place file and a script to run:

```bash
run-in-roblox --place MyPlace.rbxlx --script starter-script.lua
```

This will open `MyPlace.rbxlx` in Roblox Studio, run `starter-script.lua` until it completes, and then exit.

`--place` is optional, but `--script` is required.

## License
run-in-roblox is available under the terms of the MIT License. See [LICENSE.txt](LICENSE.txt) or <https://opensource.org/licenses/MIT> for details.