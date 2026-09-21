# Rune DSL (Rosetta) support for VS Code — powered by sigil

This extension adds language support for `.rosetta` files (the Rune DSL of
the [FINOS rune-dsl](https://github.com/finos/rune-dsl) project) to VS Code:

- **Language server** ([sigil-lsp](../../../docs/lsp.md)): diagnostics,
  go-to-definition, hover, completion, document symbols, workspace symbols,
  find references, incremental sync.
- **Syntax highlighting** via a TextMate grammar.
- **Editing niceties**: comment toggling (`//`, `/* */`), bracket
  auto-closing, indentation rules for `type`/`enum`/`choice` bodies.

## Building sigil-lsp

The extension spawns the `sigil-lsp` binary. Build it from the repository
root:

```sh
cargo build -p sigil-lsp
# binary at target/debug/sigil-lsp
cargo build --release -p sigil-lsp
# binary at target/release/sigil-lsp
```

## Using the extension during development

1. `code --new-window editors/vscode` (or open `editors/vscode` and press
   F5 after `npm install` to run an Extension Development Host).
2. Point the `sigil.server.path` setting at your built binary:

```jsonc
// .vscode/settings.json
{
  "sigil.server.path": "/absolute/path/to/target/release/sigil-lsp"
}
```

The default resolves `sigil-lsp` from `PATH`, so `cargo install
--path crates/sigil-lsp` also works.

## Packaging (vsce)

```sh
cd editors/vscode
npm install          # pulls vscode-languageclient
npx @vscode/vsce package
# produces rosetta-sigil-0.1.0.vsix
code --install-extension rosetta-sigil-0.1.0.vsix
```

## Settings

| Setting | Default | Meaning |
| --- | --- | --- |
| `sigil.server.path` | `sigil-lsp` | Binary path or command name |
| `sigil.server.args` | `[]` | Extra arguments for the server |
| `sigil.trace.server` | `off` | LSP wire tracing in the output panel |

## Notes

- The server analyzes the whole workspace together (all open documents plus
  `*.rosetta` files discovered under the workspace root at startup) — see
  [docs/lsp.md](../../docs/lsp.md) for the architecture and limitations.
