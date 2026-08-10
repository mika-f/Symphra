# Testing the Symphra LSP

The current LSP communicates over standard input and output, accepts
full-document synchronization, publishes lexical, syntax, or compiler
diagnostics, exposes document symbols, and offers context-aware keyword
completion plus song-local declared-name completion at reference sites,
documents language keywords on hover, and shows the compiled MIDI note number
for valid written pitches. Definition navigation resolves arrangement pattern
and instrument references, `arrangement { play <section> }` to section
declarations, `play track <name>` inside sections to track declarations,
track-body instrument names, `play <pattern>` (including inside layered `use`
blocks) to pattern declarations, and `trigger_with <rhythm>` to rhythm
declarations. Find-all-references, document highlight, declaration CodeLens
(`N references`), and rename / prepareRename cover the same song-local named
symbols. Semantic tokens color keywords, declared/referenced names, strings,
numbers, comments, and pitch identifiers. Inlay hints show compiled MIDI values
after pitches and a kind label after resolved name references. Its JSON-RPC
lifecycle is covered by an end-to-end stdio test.
A Visual Studio Code extension lives at
[`editors/vscode`](https://github.com/mika-f/Symphra/tree/main/editors/vscode)
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
keyword completion, keyword hover, definition navigation, references, document
highlight, code lens, prepareRename, rename, semantic tokens, inlay hints,
shutdown, and exit. The syntax tests cover conversion in both directions between
UTF-8 byte offsets and UTF-16 line and column positions, including an emoji and
invalid code-unit boundaries.

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
   include `note`. After `play` in a track that already declares patterns, those
   pattern names should appear alongside `drum`. After `instrument` in a track
   body, declared instrument names should appear; after `trigger_with`, rhythm
   names; after `play track` in a section, track names.
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
12. With `instrument lead = triangle` and a track body that contains
    `instrument lead` (or `use lead` inside `layer`), put the cursor on that
    instrument name and run `:lua vim.lsp.buf.definition()`. The cursor should
    move to the matching `instrument lead` declaration in the same song.
13. With `pattern melody = sequence {}` and a track body that contains
    `play melody` (or `use lead { play melody }` inside `layer`), put the
    cursor on `melody` and run `:lua vim.lsp.buf.definition()`. The cursor
    should move to the matching `pattern melody` declaration in the same song.
14. With a `rhythm stabs ...` declaration and
    `play melody |> trigger_with stabs`, put the cursor on `stabs` and run
    `:lua vim.lsp.buf.definition()`. The cursor should move to the matching
    `rhythm stabs` declaration in the same song.
15. On a pattern, instrument, rhythm, track, or section declaration name, run
    `:lua vim.lsp.buf.references()`. Every same-song use should appear; with
    `includeDeclaration`, the declaration itself is included.
16. With CodeLens enabled (`vim.lsp.codelens.refresh()` after open), each of
    those declarations should show `N references` / `1 reference` above the
    name. Zero-use declarations show `0 references`.
17. On a pattern name (declaration or use), run
    `:lua vim.lsp.buf.rename("theme")`. Every same-song occurrence of that
    pattern should update; an identically named pattern in another song must
    stay put. An invalid identifier or a same-kind name collision should be
    rejected.
18. Place the cursor on a declared name and run
    `:lua vim.lsp.buf.document_highlight()`. The declaration and every same-song
    use should highlight; another song's identical name must not.
19. With semantic highlighting enabled, keywords, pattern/instrument names
    (declaration vs use), strings, numbers, comments, and pitches such as `C4`
    should receive distinct semantic token classes from the server.
20. With inlay hints enabled, a compiled pitch such as `C4` should show
    `MIDI 60` after the name, and a resolved reference such as arrangement
    `melody` should show a `pattern` label.

Diagnostics should refresh after every edit because the server requests
full-text synchronization. Closing the buffer sends `textDocument/didClose`,
which clears published diagnostics.

## Current test boundary

The stdio test verifies the server protocol independently of any editor. The
manual procedure remains useful for confirming editor presentation and client
compatibility. There are no tests yet for malformed JSON-RPC frames, concurrent
documents, cancellation, or editor-specific configuration because the current
server does not need custom handling for those concerns.
