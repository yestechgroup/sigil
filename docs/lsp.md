# sigil-lsp — the Rune DSL language server

`sigil-lsp` is a language server (LSP 3.17) for `.rosetta` files, built on
[`lsp-server`](https://crates.io/crates/lsp-server) (rust-analyzer's sync
scaffold) and [`lsp-types`](https://crates.io/crates/lsp-types). It reuses
the sigil toolchain: `sigil-syntax` for parsing and `sigil-resolve` for
name resolution, so editor diagnostics match `sigil check` exactly.

## Running

```sh
cargo build -p sigil-lsp
target/debug/sigil-lsp          # speaks LSP over stdio
```

The server is transport-agnostic by design: the library exposes
`run_server(connection, world)` and the binary only wires it to stdio. The
entire test suite runs against in-memory `Connection::memory()` pairs, plus
one end-to-end test over real stdio framing.

## Capabilities

| Capability | Details |
| --- | --- |
| Position encoding | Negotiates `positionEncoding: "utf-8"` (LSP 3.17) when the client offers `general.positionEncodings`; falls back to the LSP default UTF-16. All span math happens in bytes and converts at the edges. |
| Text sync | `textDocumentSync`: open/close + **INCREMENTAL** (range-based) changes, applied in order, batches supported. |
| publishDiagnostics | Syntax (`E0001`, `W0001`, ...) and resolution (`E01xx`) diagnostics per open document, re-published after every change. |
| documentSymbol | Full tree: namespace → elements → attributes / enum values, with exact selection ranges. |
| definition | On type references (attribute types, `extends`, type-alias/record/library types, configuration roots), annotation references (`[metadata id]`), and declarations themselves. Cross-file, and into the built-in `com.rosetta.model` library (location URI `builtin:<file>`). |
| hover | Markdown: `**kind** \`fqn\`` plus the `<"...">` definition; attributes add the resolved type FQN and cardinality. Hovering a reference shows the target. |
| completion | Context-aware: type names (incl. builtins) in type-reference positions, annotation attributes inside `[metadata `, annotation names after `[`, `displayName` in enum bodies, element keywords at top level. |
| references | All sites resolving to the element under the cursor (declaration optional), across the workspace. |
| workspace/symbol | Case-insensitive substring search over fully-qualified names. |

## Architecture

- **`World`** (crates/sigil-lsp/src/world.rs): workspace root + open
  documents (`BTreeMap`, so analysis order is deterministic) + current
  `Analysis`.
- **Text sync**: `didChange` batches are applied in order to the document
  text (ranged edits resolved against the text as mutated by preceding
  edits in the same batch). `didClose` removes the document from the
  workspace.
- **Analysis**: on every change the *entire workspace* is re-parsed and
  re-resolved together (`Analysis::compute`). Models are multi-file — a
  change in one file can invalidate any other file's resolution — and a
  full reanalysis is fast enough for real models while being trivially
  consistent. The built-in `com.rosetta.model` files participate like any
  other file (they are compiled into the server via `sigil-resolve`).
- **Positions**: everything internal is byte spans (`sigil_diag::Span`).
  `position::LineIndex` converts to/from LSP positions in either the
  negotiated UTF-8 mode or the UTF-16 fallback; it clamps overshooting
  positions (common at EOF) and is property-tested for strict offset →
  position → offset bijectivity.
- **Feature queries** (features.rs) read the resolved model: references
  carry `resolved` flat ids and declaration spans, which map directly to
  `Location`s. Built-in files get `builtin:` pseudo-URIs (they are not
  workspace documents).

### Testing

The integration tests drive the server through `Connection::memory()`
pairs via a `TestClient` (tests/common/mod.rs) that performs the full
handshake, mirrors text locally, applies identical ranged edits, and
asserts diagnostics with a version-based, sleep-free poll. Two property
suites (proptest, ≥128 cases):

- incremental sync: random unicode-aware edit sequences must leave server
  text identical to the client mirror (UTF-8 and UTF-16 modes);
- position conversion: random offsets/positions round-trip in both
  encodings.

`tests/stdio_e2e.rs` spawns the actual binary and speaks the
`Content-Length` wire format to prove the stdio transport end-to-end.

Run them with:

```sh
cargo test -p sigil-lsp
PROPTEST_CASES=512 cargo test -p sigil-lsp   # longer property runs
```

## Editor setup

### VS Code

See [editors/vscode](../editors/vscode/README.md) for the packaged
extension (server client + syntax highlighting).

### Neovim (nvim-lspconfig, 0.11+ style)

```lua
vim.lsp.config("sigil_lsp", {
  cmd = { "sigil-lsp" },
  filetypes = { "rosetta" },
})
vim.lsp.enable("sigil_lsp")
```

(Older nvim-lspconfig: add a custom `lspconfig` config with
`cmd = { "sigil-lsp" }`, `filetypes = { "rosetta" }`.)

### Helix

```toml
# ~/.config/helix/languages.toml
[[language]]
name = "rosetta"
scope = "source.rosetta"
file-types = ["rosetta"]
language-servers = ["sigil-lsp"]
comment-tokens = ["//"]
indent = { tab-width = 4, unit = "    " }

[language-server.sigil-lsp]
command = "sigil-lsp"
```

### Emacs (eglot)

```elisp
(add-to-list 'eglot-server-programs '(rosetta-mode . ("sigil-lsp")))
(define-derived-mode rosetta-mode prog-mode "Rosetta"
  (setq-local comment-start "// ")
  (setq-local comment-end ""))
(add-to-list 'auto-mode-alist '("\\.rosetta\\'" . rosetta-mode))
```

## Limitations

- Files are analyzed when open in the editor (or discovered under the
  workspace root at startup). A file created on disk *after* startup is
  not analyzed until opened; `didClose` removes a document from the
  workspace rather than re-reading it from disk.
- `file://` URIs are used as-is (no percent-encoding/decoding): paths with
  spaces or non-ASCII characters will not round-trip.
- Definitions into the built-in library point at `builtin:` pseudo-URIs
  that editors cannot open (the range is still correct for tooling).
- Completion is positional heuristics over line prefixes plus the syntax
  AST — no expression-level awareness (the parser currently skips
  expressions with `W0001`).
- Only the model subset of the language is resolved; `func`, rules,
  reports and expressions are recognized-but-skipped constructs (they
  appear in diagnostics as `W0001`).
