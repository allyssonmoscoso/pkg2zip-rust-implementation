//! Arnés de paridad C vs Rust.
//!
//! Ejecuta el binario C congelado (`legacy/`) y el binario Rust sobre el mismo fixture, en
//! directorios temporales separados, y compara:
//!
//! * código de salida,
//! * árbol de archivos resultante (rutas + SHA-256 por archivo),
//! * stdout normalizado (`-l` se compara de forma estricta; en `-x` las diferencias de mensajes se
//!   registran como *warnings* hasta que la CLI Rust esté completa en F2).
//!
//! Un código de salida [`RUST_PENDING_EXIT_CODE`] en el binario Rust marca el caso como *pendiente*
//! (funcionalidad aún no migrada), no como fallo.

use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use walkdir::WalkDir;

/// Error genérico del arnés (herramienta de desarrollo).
pub type Error = Box<dyn StdError + Send + Sync>;
/// Resultado del arnés.
pub type Result<T> = std::result::Result<T, Error>;

/// Código de salida del binario Rust que significa "aún no implementado".
pub const RUST_PENDING_EXIT_CODE: i32 = 3;

/// Resultado de un caso.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// C y Rust coinciden.
    Pass,
    /// Rust todavía no implementa esta ruta.
    Pending,
    /// Hay diferencias.
    Fail,
}

/// Resultado de un fixture con un conjunto de flags.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseResult {
    /// Nombre del archivo PKG.
    pub fixture: String,
    /// Nombre del caso (`list`, `extract`).
    pub case: String,
    /// Veredicto.
    pub status: Status,
    /// Diferencias que hacen fallar el caso.
    pub details: Vec<String>,
    /// Diferencias toleradas por ahora (se endurecen en fases posteriores).
    pub warnings: Vec<String>,
}

/// Resumen completo de una corrida.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    /// Versión del testkit que generó el reporte.
    pub tool_version: String,
    /// Ruta del binario C.
    pub legacy: String,
    /// Ruta del binario Rust.
    pub rust: String,
    /// Casos ejecutados.
    pub cases: Vec<CaseResult>,
    /// Casos que coinciden.
    pub passed: usize,
    /// Casos pendientes.
    pub pending: usize,
    /// Casos con diferencias.
    pub failed: usize,
}

/// Opciones del arnés.
#[derive(Debug, Clone)]
pub struct Options {
    /// Directorio con fixtures `.pkg`.
    pub corpus: PathBuf,
    /// Binario C de referencia.
    pub legacy: PathBuf,
    /// Binario Rust.
    pub rust: PathBuf,
    /// Ruta del reporte JSON a escribir.
    pub report: PathBuf,
}

struct Case {
    name: &'static str,
    args: &'static [&'static str],
}

const CASES: &[Case] = &[
    Case {
        name: "list",
        args: &["-l"],
    },
    Case {
        name: "extract",
        args: &["-x"],
    },
];

#[derive(Debug)]
struct Outcome {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn normalize(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

fn run_bin(bin: &Path, args: &[&str], pkg: &Path, cwd: &Path) -> Result<Outcome> {
    if !bin.is_file() {
        return Err(format!("no existe el binario {}", bin.display()).into());
    }
    let output = Command::new(bin)
        .args(args)
        .arg(pkg)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("no se pudo ejecutar {}: {e}", bin.display()))?;

    Ok(Outcome {
        code: output.status.code(),
        stdout: normalize(&output.stdout),
        stderr: normalize(&output.stderr),
    })
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(hex, "{byte:02x}").expect("escritura a String no falla");
    }
    Ok(hex)
}

/// Fotografía del árbol: `d:<ruta>` para directorios, `f:<ruta>` -> sha256 para archivos.
fn snapshot(dir: &Path) -> Result<BTreeMap<String, String>> {
    let mut map = BTreeMap::new();
    for entry in WalkDir::new(dir).min_depth(1).follow_links(false) {
        let entry = entry?;
        let rel = entry
            .path()
            .strip_prefix(dir)?
            .to_string_lossy()
            .replace('\\', "/");
        if entry.file_type().is_dir() {
            map.insert(format!("d:{rel}"), String::new());
        } else if entry.file_type().is_file() {
            map.insert(format!("f:{rel}"), sha256_file(entry.path())?);
        }
    }
    Ok(map)
}

fn diff_snapshots(
    left: &BTreeMap<String, String>,
    right: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut diffs = Vec::new();
    for (key, left_hash) in left {
        match right.get(key) {
            None => diffs.push(format!("solo en C: {key}")),
            Some(right_hash) if right_hash != left_hash => {
                diffs.push(format!("contenido distinto: {key}"));
            }
            Some(_) => {}
        }
    }
    for key in right.keys() {
        if !left.contains_key(key) {
            diffs.push(format!("solo en Rust: {key}"));
        }
    }
    diffs
}

fn truncate(mut details: Vec<String>, max: usize) -> Vec<String> {
    if details.len() > max {
        let rest = details.len() - max;
        details.truncate(max);
        details.push(format!("... y {rest} diferencias más"));
    }
    details
}

fn is_pending(outcome: &Outcome) -> bool {
    outcome.code == Some(RUST_PENDING_EXIT_CODE)
}

/// Ejecuta la matriz completa y escribe el reporte JSON.
///
/// # Errors
///
/// Si no se puede leer el corpus, ejecutar un binario o escribir el reporte.
pub fn run(options: &Options) -> Result<Summary> {
    let corpus = options
        .corpus
        .canonicalize()
        .map_err(|e| format!("corpus inválido {}: {e}", options.corpus.display()))?;

    let mut fixtures: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(&corpus)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "pkg") {
            fixtures.push(path.canonicalize()?);
        }
    }
    fixtures.sort();
    if fixtures.is_empty() {
        return Err(format!("no hay fixtures .pkg en {}", corpus.display()).into());
    }

    let mut cases = Vec::new();

    for fixture in &fixtures {
        let fixture_name = fixture.file_name().map_or_else(
            || fixture.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );

        for case in CASES {
            let legacy_dir = TempDir::new()?;
            let rust_dir = TempDir::new()?;

            let legacy = run_bin(&options.legacy, case.args, fixture, legacy_dir.path())?;
            let rust = run_bin(&options.rust, case.args, fixture, rust_dir.path())?;

            let mut details = Vec::new();
            let mut warnings = Vec::new();

            let status = if is_pending(&rust) {
                Status::Pending
            } else if legacy.code != rust.code {
                details.push(format!(
                    "exit code: C={:?} Rust={:?}",
                    legacy.code, rust.code
                ));
                if !rust.stderr.is_empty() {
                    details.push(format!("stderr Rust: {}", rust.stderr.trim()));
                }
                Status::Fail
            } else if legacy.code == Some(0) {
                if case.name == "list" {
                    if legacy.stdout != rust.stdout {
                        details.push(format!(
                            "stdout distinto: C={:?} Rust={:?}",
                            legacy.stdout.trim_end(),
                            rust.stdout.trim_end()
                        ));
                        Status::Fail
                    } else {
                        Status::Pass
                    }
                } else {
                    let left = snapshot(legacy_dir.path())?;
                    let right = snapshot(rust_dir.path())?;
                    if left.is_empty() {
                        details.push("el binario C no generó archivos".to_owned());
                    }
                    let mut diffs = diff_snapshots(&left, &right);
                    if diffs.is_empty() {
                        if legacy.stdout != rust.stdout {
                            warnings.push("stdout distinto (se endurece en F2)".to_owned());
                        }
                        Status::Pass
                    } else {
                        details.append(&mut diffs);
                        Status::Fail
                    }
                }
            } else {
                // Ambos fallan con el mismo código: por ahora se acepta.
                if legacy.stderr != rust.stderr {
                    warnings.push("stderr distinto en caso de error".to_owned());
                }
                Status::Pass
            };

            cases.push(CaseResult {
                fixture: fixture_name.clone(),
                case: case.name.to_owned(),
                status,
                details: truncate(details, 15),
                warnings,
            });
        }
    }

    let passed = cases.iter().filter(|c| c.status == Status::Pass).count();
    let pending = cases.iter().filter(|c| c.status == Status::Pending).count();
    let failed = cases.iter().filter(|c| c.status == Status::Fail).count();

    let summary = Summary {
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        legacy: options.legacy.display().to_string(),
        rust: options.rust.display().to_string(),
        cases,
        passed,
        pending,
        failed,
    };

    if let Some(parent) = options.report.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let mut report = File::create(&options.report)?;
    serde_json::to_writer_pretty(&mut report, &summary)?;
    report.write_all(b"\n")?;

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(code: i32) -> Outcome {
        Outcome {
            code: Some(code),
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    #[test]
    fn pending_exit_code_is_detected() {
        assert!(is_pending(&outcome(RUST_PENDING_EXIT_CODE)));
        assert!(!is_pending(&outcome(0)));
        assert!(!is_pending(&outcome(1)));
    }

    #[test]
    fn diff_reports_both_directions() {
        let mut left = BTreeMap::new();
        left.insert("f:a".to_owned(), "11".to_owned());
        left.insert("f:b".to_owned(), "22".to_owned());
        let mut right = BTreeMap::new();
        right.insert("f:b".to_owned(), "22".to_owned());
        right.insert("f:c".to_owned(), "33".to_owned());

        let diffs = diff_snapshots(&left, &right);
        assert!(diffs.iter().any(|d| d.contains("solo en C: f:a")));
        assert!(diffs.iter().any(|d| d.contains("solo en Rust: f:c")));
        assert!(!diffs.iter().any(|d| d.contains("f:b")));
    }

    #[test]
    fn snapshot_includes_dirs_and_files() {
        let dir = TempDir::new().expect("tempdir");
        fs::create_dir(dir.path().join("sub")).expect("mkdir");
        fs::write(dir.path().join("sub/file.bin"), b"hola").expect("write");

        let map = snapshot(dir.path()).expect("snapshot");
        assert!(map.contains_key("d:sub"));
        assert_eq!(map.get("f:sub/file.bin").map(String::len), Some(64));
    }
}
