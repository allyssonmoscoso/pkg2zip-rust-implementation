//! CLI de `pkg2zip` (reescritura en Rust).
//!
//! Fase 0: solo se reconocen `-h/--help` y `--version`. El procesamiento de PKG llega en las fases
//! F2–F4; hasta entonces se sale con código [`PENDING_EXIT_CODE`] para que el arnés de paridad
//! marque los casos como "pendientes" en vez de como fallos.

use std::process::ExitCode;

/// Código de salida que indica "funcionalidad aún no migrada".
///
/// El arnés de paridad (`pkg2zip-testkit parity`) lo interpreta como caso pendiente.
pub const PENDING_EXIT_CODE: u8 = 3;

const HELP: &str = "\
Usage: pkg2zip [-x] [-c[N]] [-b] [-p] <file.pkg> [zRIF]

Parameters:
  -x|--extract       Extract only. No zip compression
  -l|--list          Shows the package (sfo) name and exits
  -b|--no-bgdl       Disable bgdl output for VITA Theme extraction
  -q|--quiet         Do not output anything to stdout
  -h|--help          Shows this help message

PSP/PSX only options:
  -c[NUM]            Create a *.CSO file instead of ISO. [NUM] is the compression ratio
  -p|--psp           Extracts PSP files in their original EBOOT.PBP format
  -d|--decrypt       Always decrypt PSP DLC/EDAT files

PSM only options:
  -a|--android-psm   Extract into PSM for Android format.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut want_help = false;
    let mut want_version = false;
    let mut positional = Vec::new();

    for arg in &args {
        match arg.as_str() {
            "-h" | "--help" => want_help = true,
            "--version" | "-V" => want_version = true,
            other if other.starts_with('-') && other.len() > 1 => {
                // F0: los flags conocidos del C se aceptan pero no se procesan todavía.
            }
            other => positional.push(other.to_owned()),
        }
    }

    if want_version {
        println!("pkg2zip {}", pkg2zip_core::VERSION);
        return ExitCode::SUCCESS;
    }

    if want_help {
        println!("pkg2zip v{}", pkg2zip_core::VERSION);
        println!();
        print!("{HELP}");
        return ExitCode::SUCCESS;
    }

    if positional.is_empty() {
        eprintln!("ERROR: no pkg file specified");
        print!("{HELP}");
        return ExitCode::from(1);
    }

    eprintln!(
        "ERROR: the Rust implementation is under construction (F0); use the legacy binary from \
         legacy/ meanwhile. See docs/PLAN-MIGRACION-RUST.md"
    );
    ExitCode::from(PENDING_EXIT_CODE)
}
