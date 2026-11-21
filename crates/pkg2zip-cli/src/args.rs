//! Parseo de argumentos compatible con el binario C (incluidas sus rarezas).

/// Configuración de una ejecución.
#[derive(Debug, Clone)]
pub(crate) struct Config {
    /// Crear zip (por defecto) o extraer (`-x`).
    pub zipped: bool,
    /// Solo mostrar el nombre (`-l`).
    pub listing: bool,
    /// Mensajes informativos (por defecto sí; `-q` lo desactiva).
    pub verbose: bool,
    /// Factor de compresión CSO (`-cN`, 0..=9; 0 significa ISO).
    pub cso: u8,
    /// Mantener EBOOT.PBP en vez de ISO (`-p`).
    pub pbp: bool,
    /// Descifrar siempre EDAT de PSP (`-d`).
    pub ddlc: bool,
    /// Salida bgdl para temas de Vita (desactivada con `-b`).
    pub bgdl: bool,
    /// Layout PSM para Android (`-a`).
    pub android: bool,
    /// Ruta del PKG.
    pub pkg: String,
    /// zRIF o ruta a `.bin`.
    pub zrif: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            zipped: true,
            listing: false,
            verbose: true,
            cso: 0,
            pbp: false,
            ddlc: false,
            bgdl: true,
            android: false,
            pkg: String::new(),
            zrif: None,
        }
    }
}

/// Resultado del parseo.
#[derive(Debug, Clone)]
pub(crate) enum Parsed {
    /// `-h/--help`.
    Help,
    /// `--version`.
    Version,
    /// Ejecución normal.
    Run(Config),
}

/// Parsea los argumentos (sin el nombre del binario).
///
/// # Errors
///
/// Devuelve el mensaje `ERROR: no pkg file specified` si no hay PKG.
pub(crate) fn parse(args: &[String]) -> Result<Parsed, String> {
    let mut config = Config::default();
    let mut pkg: Option<String> = None;
    let mut zrif: Option<String> = None;
    let mut help = false;
    let mut version = false;

    for arg in args {
        if arg == "-x" || arg == "--extract" {
            config.zipped = false;
        } else if arg == "-l" || arg == "--list" {
            config.listing = true;
            config.verbose = false;
        } else if arg.starts_with("-c") {
            config.cso = atoi_prefix(arg.strip_prefix("-c").unwrap_or_default()).clamp(0, 9) as u8;
        } else if arg == "-p" || arg == "--psp" {
            config.pbp = true;
        } else if arg == "-d" || arg == "--decrypt" {
            config.ddlc = true;
        } else if arg == "-b" || arg == "--no-bgdl" {
            config.bgdl = false;
        } else if arg == "-q" || arg == "--quiet" {
            config.verbose = false;
        } else if arg == "-a" || arg == "--android-psm" {
            config.android = true;
        } else if arg == "-h" || arg == "--help" {
            help = true;
        } else if arg == "--version" || arg == "-V" {
            version = true;
        } else if pkg.is_some() {
            if !arg.is_empty() {
                zrif = Some(arg.clone());
            }
            break;
        } else {
            pkg = Some(arg.clone());
        }
    }

    if version {
        return Ok(Parsed::Version);
    }
    if help {
        return Ok(Parsed::Help);
    }
    let pkg = pkg.ok_or_else(|| "ERROR: no pkg file specified".to_owned())?;
    config.pkg = pkg;
    config.zrif = zrif;
    Ok(Parsed::Run(config))
}

/// `atoi` de C para el prefijo numérico (`-c9foo` -> 9, `-c` -> 0).
fn atoi_prefix(text: &str) -> i32 {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut sign = 1i32;
    if bytes.first() == Some(&b'-') {
        sign = -1;
        index = 1;
    } else if bytes.first() == Some(&b'+') {
        index = 1;
    }
    let mut value = 0i32;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        value = value
            .saturating_mul(10)
            .saturating_add(i32::from(bytes[index] - b'0'));
        index += 1;
    }
    value.saturating_mul(sign)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    fn run(list: &[&str]) -> Config {
        match parse(&args(list)).expect("parse") {
            Parsed::Run(config) => config,
            other => panic!("esperaba Run, obtuve {other:?}"),
        }
    }

    #[test]
    fn defaults() {
        let config = run(&["game.pkg"]);
        assert!(config.zipped);
        assert!(config.verbose);
        assert!(config.bgdl);
        assert_eq!(config.cso, 0);
        assert_eq!(config.pkg, "game.pkg");
        assert!(config.zrif.is_none());
    }

    #[test]
    fn parses_flags_and_zrif() {
        let config = run(&["-x", "-b", "-q", "game.pkg", "ABCD"]);
        assert!(!config.zipped);
        assert!(!config.bgdl);
        assert!(!config.verbose);
        assert_eq!(config.zrif.as_deref(), Some("ABCD"));
    }

    #[test]
    fn cso_uses_atoi_prefix_and_clamps() {
        assert_eq!(run(&["-c9", "a.pkg"]).cso, 9);
        assert_eq!(run(&["-c9foo", "a.pkg"]).cso, 9);
        assert_eq!(run(&["-c", "a.pkg"]).cso, 0);
        assert_eq!(run(&["-c99", "a.pkg"]).cso, 9);
        assert_eq!(run(&["-c-3", "a.pkg"]).cso, 0);
    }

    #[test]
    fn listing_implies_quiet() {
        let config = run(&["-l", "a.pkg"]);
        assert!(config.listing);
        assert!(!config.verbose);
    }

    #[test]
    fn unknown_flag_is_a_pkg_path() {
        let config = run(&["--desconocido"]);
        assert_eq!(config.pkg, "--desconocido");
    }

    #[test]
    fn missing_pkg_is_an_error() {
        assert_eq!(
            parse(&args(&["-x"])).expect_err("sin pkg"),
            "ERROR: no pkg file specified"
        );
    }

    #[test]
    fn help_and_version() {
        assert!(matches!(parse(&args(&["-h"])), Ok(Parsed::Help)));
        assert!(matches!(parse(&args(&["--version"])), Ok(Parsed::Version)));
    }
}
