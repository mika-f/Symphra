# Testing the Symphra LSP

The current LSP communicates over standard input and output, accepts
full-document synchronization, publishes lexical, syntax, or compiler
diagnostics, exposes document symbols, and offers context-aware keyword
completion, documents language keywords on hover, and shows the compiled MIDI
note number for valid written pitches. Definition navigation resolves
arrangement pattern and instrument references, `arrangement { play <section> }`
to section declarations, and `play track <name>` inside sections to track
declarations. Its JSON-RPC lifecycle is covered by an end-to-end stdio test.
A Visual Studio Code extension lives at [`editors/vscode`](../../editors/vscode)
and provides syntax highlighting plus a language client for manual testing;
see that directory's README for setup.

## Automated verification

Run the focused tests from the repository root:

```console
cargo test -p symphra-lsp --locked
cargo test -p symphra-syntax --locked
```

The LSP unit tests cover syntax and compiler diagnostic selection. The stdio
integration test launches the built binary and covers initialize, open,
full-text change, close, published diagnostics, hierarchical document symbols,
keyword completion, keyword hover, definition navigation, shutdown, and
exit. The syntax tests cover
conversion in both directions between UTF-8 byte offsets and UTF-16 line and
column positions, including an emoji and invalid code-unit boundaries.

Before committing an LSP change, run the complete acceptance checks:

```console
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

These commands need access to crates.io unless all locked dependencies are
already cached. Add `--offline` when intentionally testing from the cache.

## Manual editor test with Neovim

Build the debug binary from the repository root:

```console
cargo build -p symphra-lsp --locked
```

Open a `.sym` file in Neovim from the same directory. Then execute:

```vim
:set filetype=symphra
:lua local suffix = vim.fn.has("win32") == 1 and ".exe" or ""; vim.lsp.start({ name = "symphra", cmd = { vim.fn.getcwd() .. "/target/debug/symphra-lsp" .. suffix }, root_dir = vim.fn.getcwd() })
```

Use `:checkhealth vim.lsp` to inspect the client connection. Depending on the
Neovim version, `:LspInfo` may also be available.

Exercise these cases by replacing the buffer contents without saving:

1. Put `@` at the beginning of the file. A lexical error should cover that
   character.
2. Use `project { seed nope }`. Syntax diagnostics should be reported, without
   compiler diagnostics being mixed in.
3. Use the following parseable source. The compiler should report `key is
   required`:

   ```symphra
   project { seed 1 sample_rate 48khz output stereo }
   song "Test" {
     tempo 120bpm
     meter 4/4
     pattern melody = sequence {}
   }
   ```

4. Add `key C major` inside the song. The diagnostic should disappear.
5. Put an emoji before an error and confirm the underline remains aligned. LSP
   columns count UTF-16 code units, not UTF-8 bytes.
6. Open the editor's outline or symbol picker. It should show `project` and each
   song at the top level, with patterns nested under their song.
7. On an empty top-level line, request completion (normally `<C-x><C-o>` in
   insert mode). It should include `project` and `song`. Inside a song it should
   include `tempo`, `meter`, `key`, and `pattern`; inside a sequence it should
   include `note`.
8. Place the cursor on a language keyword and run `:lua vim.lsp.buf.hover()`.
   A short Markdown description should appear and apply to exactly that token.
9. With a semantically valid document, hover `C4`. It should report MIDI note
   60. Semantic pitch help is intentionally absent while the document cannot be
   compiled successfully.
10. Add `arrangement { melody }`, put the cursor on `melody`, and run
    `:lua vim.lsp.buf.definition()`. The cursor should move to the name in the
    corresponding `pattern melody` declaration in the same song.
11. With a `track pad ...` declaration and
    `section intro bars 2 { parallel { play track pad } }`, put the cursor on
    `pad` in `play track pad` and run `:lua vim.lsp.buf.definition()`. The
    cursor should move to the name in the corresponding `track pad` declaration
    in the same song.

Diagnostics should refresh after every edit because the server requests
full-text synchronization. Closing the buffer sends `textDocument/didClose`,
which clears published diagnostics.

## Current test boundary

The stdio test verifies the server protocol independently of any editor. The
manual procedure remains useful for confirming editor presentation and client
compatibility. There are no tests yet for malformed JSON-RPC frames, concurrent
documents, cancellation, or editor-specific configuration because the current
server does not need custom handling for those concerns.
