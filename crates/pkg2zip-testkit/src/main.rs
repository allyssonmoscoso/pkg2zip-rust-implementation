//! Herramienta de desarrollo `pkg2zip-testkit`.
//!
//! ```text
//! pkg2zip-testkit gen <dir>                 # escribe <dir>/vita_app.pkg
//! pkg2zip-testkit parity --corpus <dir> --legacy <bin> --rust <bin> --report <json>
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use pkg2zip_testkit::parity::{self, Options, Status};
use pkg2zip_testkit::pkg::{DEFAULT_CONTENT_ID, build_vita_app};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("gen") => gen_cmd(&args[1..]),
        Some("parity") => parity_cmd(&args[1..]),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(code) => code,
        Err(message) => {
            eprintln!("ERROR: {message}");
            ExitCode::from(1)
        }
    }
}

const USAGE: &str = "\
usage:
  pkg2zip-testkit gen <dir>
  pkg2zip-testkit parity --corpus <dir> --legacy <bin> --rust <bin> --report <json>
";

fn gen_cmd(args: &[String]) -> Result<ExitCode, String> {
    let dir = args.first().ok_or("falta el directorio de salida")?;
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("no se pudo crear {}: {e}", dir.display()))?;

    let pkg = build_vita_app("Synthetic Vita App", DEFAULT_CONTENT_ID);
    let path = dir.join("vita_app.pkg");
    std::fs::write(&path, &pkg.bytes)
        .map_err(|e| format!("no se pudo escribir {}: {e}", path.display()))?;

    println!("{} ({} bytes)", path.display(), pkg.bytes.len());
    Ok(ExitCode::SUCCESS)
}

fn flag<'a>(args: &'a [String], name: &str) -> Result<&'a str, String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == name {
            return iter
                .next()
                .map(String::as_str)
                .ok_or_else(|| format!("{name} necesita un valor"));
        }
    }
    Err(format!("falta {name}"))
}

fn parity_cmd(args: &[String]) -> Result<ExitCode, String> {
    let options = Options {
        corpus: PathBuf::from(flag(args, "--corpus")?),
        legacy: PathBuf::from(flag(args, "--legacy")?),
        rust: PathBuf::from(flag(args, "--rust")?),
        report: PathBuf::from(flag(args, "--report")?),
    };

    let summary = parity::run(&options).map_err(|e| e.to_string())?;

    println!(
        "parity: {} passed, {} pending, {} failed  (reporte: {})",
        summary.passed,
        summary.pending,
        summary.failed,
        options.report.display()
    );

    for case in &summary.cases {
        if case.status == Status::Fail {
            println!("FAIL {} / {}", case.fixture, case.case);
            for detail in &case.details {
                println!("      - {detail}");
            }
        }
    }

    if summary.failed > 0 {
        Ok(ExitCode::from(1))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}
