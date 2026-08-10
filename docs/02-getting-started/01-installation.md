# Installation

Symphra is a Rust workspace. There is no published crate on crates.io yet —
build from source.

## Requirements

- **Rust 1.88+** with Cargo (edition 2024)
- A C toolchain if your platform needs one for native dependencies (VST3 host
  path on some targets)

## Clone and build

```console
git clone https://github.com/mika-f/Symphra.git
cd Symphra
cargo build --workspace --locked
```

Useful packages:

| Package | Binary / crate | Role |
| --- | --- | --- |
| `symphra` | `symphra` | Compile `.sym` → WAV |
| `symphra-lsp` | `symphra-lsp` | Language server (stdio) |
| `symphra-formatter` | `symphra-formatter` | Format source |

Release builds:

```console
cargo build -p symphra --release --locked
cargo build -p symphra-lsp --release --locked
```

Binaries land under `target/debug/` or `target/release/`.

## Verify

```console
cargo test --workspace --all-targets --locked
```

On Windows, a running editor may lock `symphra-lsp.exe`. If the LSP package
fails to relink, re-run its tests with an alternate target directory:

```console
cargo test -p symphra-lsp --locked --target-dir target/lsp-test
```
