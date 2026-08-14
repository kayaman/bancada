use std::path::PathBuf;

use bancada_core::circuit;

fn usage() -> ! {
    eprintln!("Usage: bancada-circuit <init|sync|check|validate> [--project DIR] [--board ID] [--name NAME] [--fqbn FQBN] [--json]");
    std::process::exit(2);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_else(|| usage());
    let mut project = PathBuf::from(".");
    let mut board = None;
    let mut name = None;
    let mut fqbn = None;
    let mut json = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--project" => project = PathBuf::from(args.next().unwrap_or_else(|| usage())),
            "--board" => board = Some(args.next().unwrap_or_else(|| usage())),
            "--name" => name = Some(args.next().unwrap_or_else(|| usage())),
            "--fqbn" => fqbn = Some(args.next().unwrap_or_else(|| usage())),
            "--json" => json = true,
            _ => usage(),
        }
    }
    let result = match command.as_str() {
        "init" => circuit::init_project(
            &project,
            board.as_deref(),
            name.as_deref().unwrap_or("Circuit"),
            fqbn.as_deref(),
        )
        .map(|s| s.validation),
        "sync" => circuit::sync_project(&project, fqbn.as_deref()).map(|s| s.validation),
        "check" | "validate" => {
            circuit::validate_project(&project, fqbn.as_deref()).and_then(|r| {
                r.ok_or_else(|| {
                    bancada_core::Error::Other(format!("{} does not exist", circuit::MANIFEST_PATH))
                })
            })
        }
        _ => usage(),
    };
    match result {
        Ok(report) => {
            if json || command == "validate" {
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                println!(
                    "circuit: {} ({} diagnostics, {})",
                    if report.valid { "valid" } else { "blocked" },
                    report.diagnostics.len(),
                    report.manifest_digest
                );
            }
            if !report.valid {
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
