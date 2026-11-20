# mirror-displays

macOS display mirroring via
[displayplacer](https://github.com/jakehilborn/displayplacer).

Mirrors the primary display to every connected external display and keeps
a backup of the original layout, so `--restore` can undo it.

## Requirements

- macOS
- `cargo`/`rustc` (via [rustup](https://rustup.rs) if you don't have them)
- `displayplacer` (see [Install](#install-locally))

## Pull

```sh
git clone https://github.com/89jobrien/mirror-displays.git
cd mirror-displays
```

To fetch updates from an existing clone:

```sh
git pull
```

(Alternatively: `gh repo clone 89jobrien/mirror-displays`.)

## Install locally

One-liner — installs the binary into `~/.cargo/bin`:

```sh
cargo install --git https://github.com/89jobrien/mirror-displays
```

From a clone instead: `cargo install --path .`

Or build and run it from the target directory without installing:

```sh
cargo build
./target/debug/mirror-displays --help
```

### Install the displayplacer dependency

```sh
brew install displayplacer
```

`mirror-displays --install` does the same thing via Homebrew.

## Usage

```sh
mirror-displays            # mirror the primary display to all connected displays
mirror-displays --mirror   # same as the default above
mirror-displays --restore  # restore the original layout saved before mirroring
mirror-displays --list     # list connected displays
mirror-displays --install  # install the displayplacer dependency
mirror-displays --help     # usage
```

The backup of the original layout lives at
`~/.config/mirror-displays/original-profile`.

## Development

```sh
cargo test    # run the unit tests
cargo build   # build the binary
```
