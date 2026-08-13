# Installation

## Prebuilt binaries

Tagged releases publish archives of `symphra`, `symphra-player`,
`symphra-lsp`, and `symphra-formatter` for:

| Target | Archive |
| --- | --- |
| Linux x86_64 | `symphra-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` |
| Windows x86_64 | `symphra-vX.Y.Z-x86_64-pc-windows-msvc.zip` |
| macOS Apple silicon | `symphra-vX.Y.Z-aarch64-apple-darwin.tar.gz` |

Download from the
[GitHub Releases](https://github.com/mika-f/Symphra/releases) page, extract the
archive, and put the binaries on your `PATH`.

There is no published crate on crates.io yet. 

## Build from source

### Requirements

- **Rust 1.88+** with Cargo (edition 2024)
- A C toolchain if your platform needs one for native dependencies (VST3 host
  path on some targets)
- On Linux: `libasound2-dev`, `libxcb1-dev`, and `pkg-config` (VST3 host /
  `cpal` need ALSA; the same path also links XCB)

### Clone and build

```console
git clone https://github.com/mika-f/Symphra.git
cd Symphra
cargo build --workspace --locked
```

Useful packages:

| Package | Binary / crate | Role |
| --- | --- | --- |
| `symphra` | `symphra` | Compile `.sym` → WAV |
| `symphra-player` | `symphra-player` | Loop a rendered WAV in the background |
| `symphra-lsp` | `symphra-lsp` | Language server (stdio) |
| `symphra-formatter` | `symphra-formatter` | Format source |

Release builds:

```console
cargo build -p symphra --release --locked
cargo build -p symphra-player --release --locked
cargo build -p symphra-lsp --release --locked
cargo build -p symphra-formatter --release --locked
```

Binaries land under `target/debug/` or `target/release/`.

## Verify

```console
cargo test --workspace --all-targets --locked
```

CI runs the same test command (plus `cargo fmt --check` and `cargo clippy`) on
Linux, Windows, and macOS for every pull request and push to `main`.

On Windows, a running editor may lock `symphra-lsp.exe`. If the LSP package
fails to relink, re-run its tests with an alternate target directory:

```console
cargo test -p symphra-lsp --locked --target-dir target/lsp-test
```
