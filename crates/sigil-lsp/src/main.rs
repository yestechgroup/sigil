//! `sigil-lsp` — the Rune DSL language server over stdio.

use std::process::ExitCode;

fn main() -> ExitCode {
    let (connection, io_threads) = lsp_server::Connection::stdio();
    let world = sigil_lsp::World::new(None);
    let result = sigil_lsp::run_server(connection, world);
    if let Err(e) = io_threads.join() {
        eprintln!("sigil-lsp: io threads: {e}");
    }
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sigil-lsp: {e}");
            ExitCode::FAILURE
        }
    }
}
