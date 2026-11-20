# mirror-displays

macOS display mirroring, implemented natively via the public
[CoreGraphics](https://developer.apple.com/documentation/coregraphics)
API. No external dependencies — the original `displayplacer`-based
version is retired.

## Requirements

- macOS
- `cargo`/`rustc` (via [rustup](https://rustup.rs) if you don't have them)

## Pull

```sh
git clone https://github.com/89jobrien/mirror-displays.git
cd mirror-displays
```

To fetch updates from an existing clone:

```sh
git pull
```

## Install locally

One-liner — installs the binary into `~/.cargo/bin`:

```sh
cargo install mirror-displays
```

(From GitHub: `cargo install --git https://github.com/89jobrien/mirror-displays`.)

From a clone instead: `cargo install --path .`

Or build and run it from the target directory without installing:

```sh
cargo build
./target/debug/mirror-displays --help
```

## Usage

```sh
mirror-displays            # mirror the primary display to all connected displays
mirror-displays --mirror   # same as the default above
mirror-displays --restore  # restore the original layout saved before mirroring
mirror-displays --list     # list connected displays
mirror-displays --install  # nothing to install: no external dependencies
mirror-displays --help     # usage
```

The backup of the original layout lives at
`~/.config/mirror-displays/original-profile`.

## Development

```sh
cargo test    # run the unit tests
cargo build   # build the binary
```
