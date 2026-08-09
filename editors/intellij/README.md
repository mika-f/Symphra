# Symphra for IntelliJ Platform

Syntax highlighting and language server integration for Symphra (`.sym`) files,
built with the [IntelliJ Platform Gradle Plugin](https://plugins.jetbrains.com/docs/intellij/tools-intellij-platform-gradle-plugin.html)
and JetBrains' built-in [LSP client API](https://plugins.jetbrains.com/docs/intellij/language-server-protocol.html).
The grammar is still evolving (see [`docs/language/draft-0.1.md`](../../docs/language/draft-0.1.md));
this plugin tracks the current draft and will need updates as the language grows.

## Requirements

The bundled LSP client is only available in **licensed commercial IntelliJ
Platform IDEs** (e.g. IntelliJ IDEA Ultimate) — it does not exist in IntelliJ
IDEA Community Edition or Android Studio. There is no module ID that reliably
gates on this at install time (`com.intellij.modules.lsp` is not registered by
any IntelliJ Platform build tested against, 2024.2 or a licensed 2025.2
Ultimate, and declaring it as a `<depends>` only leaves the plugin permanently
disabled with "Requires plugin 'com.intellij.modules.lsp' to be installed").
On Community Edition, `SymphraLspServerDescriptor`'s use of
`com.intellij.platform.lsp.api` classes will fail to load instead. Syntax
highlighting has no such restriction, but this plugin only targets LSP-capable
IDEs since that's where it's meant to run.

Also needed to build:

- JDK 21
- The bundled Gradle wrapper (`./gradlew`) downloads Gradle itself; no local
  Gradle install is required.

Minimum target platform: **2025.2** (build `252`), matching `gradle.properties`'
`platformVersion` and `build.gradle`'s `sinceBuild` — both must stay in sync,
since `SymphraLspServerDescriptor` overrides `lspFormattingSupport`, whose
presence on older builds hasn't been verified.

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
- A minimal `ParserDefinition` ([`SymphraParserDefinition`](src/main/kotlin/dev/symphra/idea/psi/SymphraParserDefinition.kt))
  that wraps the raw lexer token stream in one flat PSI file with no real
  grammar structure. It exists only so `.sym` files are recognized as PSI
  files of the Symphra language rather than generic plain text, which some
  IDE features key off of.
- A **Format Symphra Document** action (`Edit` menu, or `Find Action`), which
  pipes the current document through `symphra-formatter -`. This is
  deliberately not wired into the built-in "Reformat Code" (Ctrl+Alt+L):
  that action never sends `textDocument/formatting` to the language server
  for this plugin in practice, even with `LspServerDescriptor.lspFormattingSupport`
  set and the server correctly advertising `documentFormattingProvider` —
  confirmed by having the server log every formatting request it received,
  which stayed empty across repeated attempts. Spawning the formatter
  directly sidesteps that gap entirely.

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

Build the language server and formatter from the repository root:

```console
cargo build -p symphra-lsp -p symphra-formatter --locked
```

Then open a `.sym` file (e.g.
[`examples/draft-0.1/001-sample.sym`](../../examples/draft-0.1/001-sample.sym))
in the sandbox instance.

## Locating the language server and formatter

By default the plugin looks for `target/debug/symphra-lsp` or
`target/release/symphra-lsp` under the project root for the language server,
and the equivalent `symphra-formatter` path for the **Format Symphra
Document** action, then falls back to each name on `PATH`. Set either path
under **Settings \| Tools \| Symphra** to point at a specific binary if that
doesn't find it.
