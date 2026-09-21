// sigil-lsp client for VS Code. Dependency-light: only vscode-languageclient.
const {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} = require("vscode-languageclient");
const vscode = require("vscode");

let client;

function activate(context) {
  const config = vscode.workspace.getConfiguration("sigil");
  const serverPath = config.get("server.path", "sigil-lsp");
  const serverArgs = config.get("server.args", []);

  const serverOptions = {
    run: { command: serverPath, args: serverArgs, transport: TransportKind.stdio },
    debug: { command: serverPath, args: serverArgs, transport: TransportKind.stdio },
  };

  const clientOptions = {
    // Register the server for .rosetta documents.
    documentSelector: [
      { language: "rosetta", scheme: "file" },
      { language: "rosetta", scheme: "untitled" },
    ],
    synchronize: {
      // Keep the sigil.* settings in sync with the server (future use).
      configurationSection: "sigil",
    },
    outputChannelName: "sigil-lsp",
  };

  client = new LanguageClient(
    "sigil-lsp",
    "Rune DSL (sigil)",
    serverOptions,
    clientOptions
  );
  context.subscriptions.push(client.start());
}

function deactivate() {
  if (client) {
    return client.stop();
  }
  return undefined;
}

module.exports = { activate, deactivate };
