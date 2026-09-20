//! `sigil` — command-line interface for the Rune DSL toolchain.

use std::process::ExitCode;

use sigil_diag::{Diagnostic, SourceFile};
use sigil_model::ModelFile;
use sigil_resolve::{canonical_json, resolve};

#[derive(PartialEq)]
enum Command {
    Parse,
    Check,
    Model,
}

fn usage() -> &'static str {
    "sigil — Rune DSL toolchain

USAGE:
    sigil parse <files...>    parse each file, report syntax diagnostics
    sigil check <files...>    parse and resolve, report all diagnostics
    sigil model <files...>    parse and resolve, print the canonical model JSON
"
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprint!("{}", usage());
        return ExitCode::from(2);
    }
    let command = match args[0].as_str() {
        "parse" => Command::Parse,
        "check" => Command::Check,
        "model" => Command::Model,
        other => {
            eprintln!("unknown command: {other}\n\n{}", usage());
            return ExitCode::from(2);
        }
    };

    let mut files = Vec::new();
    let mut failed_read = false;
    for path in &args[1..] {
        match std::fs::read_to_string(path) {
            Ok(text) => files.push(SourceFile::new(path, text)),
            Err(e) => {
                eprintln!("sigil: cannot read {path}: {e}");
                failed_read = true;
            }
        }
    }
    if failed_read {
        return ExitCode::from(2);
    }

    match run(command, &files) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("sigil: {message}");
            ExitCode::from(2)
        }
    }
}

fn run(command: Command, files: &[SourceFile]) -> Result<ExitCode, String> {
    let mut units = Vec::new();
    let mut syntax_diagnostics: Vec<Diagnostic> = Vec::new();
    for file in files {
        let (unit, mut diags) = sigil_syntax::parse(file);
        syntax_diagnostics.append(&mut diags);
        if let Some(unit) = unit {
            units.push((file.name.clone(), unit));
        }
    }

    let has_syntax_errors = syntax_diagnostics
        .iter()
        .any(|d| d.severity == sigil_diag::Severity::Error);
    if has_syntax_errors && matches!(command, Command::Check | Command::Model) {
        render(files, &syntax_diagnostics);
        return Ok(ExitCode::FAILURE);
    }

    match command {
        Command::Parse => {
            render(files, &syntax_diagnostics);
            Ok(if has_syntax_errors {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            })
        }
        Command::Check | Command::Model => {
            let models: Vec<ModelFile> = units
                .iter()
                .map(|(name, unit)| sigil_syntax::lower(name, unit))
                .collect();
            let resolution = resolve(models);

            if command == Command::Model {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&canonical_json(&resolution))
                        .map_err(|e| e.to_string())?
                );
                let has_errors = resolution
                    .diagnostics
                    .iter()
                    .any(|d| d.severity == sigil_diag::Severity::Error);
                return Ok(if has_errors {
                    ExitCode::FAILURE
                } else {
                    ExitCode::SUCCESS
                });
            }

            render(files, &syntax_diagnostics);
            let has_errors = syntax_diagnostics
                .iter()
                .any(|d| d.severity == sigil_diag::Severity::Error)
                || resolution
                    .diagnostics
                    .iter()
                    .any(|d| d.severity == sigil_diag::Severity::Error);
            Ok(if has_errors {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            })
        }
    }
}

fn render(files: &[SourceFile], diagnostics: &[Diagnostic]) {
    if diagnostics.is_empty() {
        return;
    }
    let texts: Vec<(String, String)> = files
        .iter()
        .map(|f| (f.name.clone(), f.text.clone()))
        .collect();
    for d in diagnostics {
        println!("{}", d.render(&texts));
    }
}
