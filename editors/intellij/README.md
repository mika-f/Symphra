# Symphra for IntelliJ Platform

Syntax highlighting and language server integration for Symphra (`.sym`) files,
built with the [IntelliJ Platform Gradle Plugin](https://plugins.jetbrains.com/docs/intellij/tools-intellij-platform-gradle-plugin.html)
and JetBrains' built-in [LSP client API](https://plugins.jetbrains.com/docs/intellij/language-server-protocol.html).
The grammar is still evolving (see [`docs/language/draft-0.1.md`](../../docs/language/draft-0.1.md));
this plugin tracks the current draft and will need updates as the language grows.

## Requirements

The bundled LSP client is only available in **licensed commercial IntelliJ
Platform IDEs** (e.g. IntelliJ IDEA Ultimate) — it does not exist in IntelliJ
IDEA Community Edition or Android Studio. `plugin.xml` declares a hard
`<depends>com.intellij.modules.lsp</depends>`, so the plugin simply won't load
on an unsupported product. Syntax highlighting has no such restriction, but
this plugin only targets LSP-capable IDEs since that's where it's meant to run.

Also needed to build:

- JDK 21
- The bundled Gradle wrapper (`./gradlew`) downloads Gradle itself; no local
  Gradle install is required.

Minimum target platform: **2024.2** (build `242`).

## What it provides

- A hand-written lexer ([`SymphraLexer`](src/main/kotlin/dev/symphra/idea/lexer/SymphraLexer.kt))
  covering the Draft 0.1 token classes — comments, strings, numbers, rate
  literals (`48khz`, `150bpm`), pitch literals (`C4`), keywords, and
  punctuation — mirroring the VS Code TextMate grammar
  (`editors/vscode/syntaxes/symphra.tmLanguage.json`) rather than a formal
  parser, since the grammar is still changing. A colors page under
  **Settings \| Editor \| Color Scheme \| Symphra** lets you customize each
  token's styling.
- An `LspServerSupportProvider` that launches `symphra-lsp` over stdio for
  `.sym` files and forwards diagnostics, completion, hover, and document
  symbols, using the platform's own LSP client (no third-party dependency).

## Building and running

From this directory:

```console
./gradlew buildPlugin
```

This produces an installable ZIP under `build/distributions/`. To try the
plugin in a sandboxed IDE instance instead:

```console
./gradlew runIde
```

Build the language server from the repository root:

```console
cargo build -p symphra-lsp --locked
```

Then open a `.sym` file (e.g.
[`examples/draft-0.1/001-sample.sym`](../../examples/draft-0.1/001-sample.sym))
in the sandbox instance.

## Locating the language server

By default the plugin looks for `target/debug/symphra-lsp` or
`target/release/symphra-lsp` under the project root, then falls back to
`symphra-lsp` on `PATH`. Set a path under **Settings \| Tools \| Symphra** to
point at a specific binary if that doesn't find it.
