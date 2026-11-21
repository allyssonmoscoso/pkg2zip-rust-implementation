//! CLI de `pkg2zip` (reescritura en Rust).
//!
//! Fase F2: Vita APP/PATCH/DLC/THEME completos (zip, `-x`, `-l`, `work.bin`, extras y temas bgdl).
//! PSP/PSX/PS3 llegan en F3 y PSM en F4; hasta entonces se sale con código 3 ("pendiente") para
//! que el arnés de paridad lo distinga de un fallo.

mod args;
mod ui;

use std::path::Path;
use std::process::ExitCode;

use pkg2zip_core::crypto::derive_keys;
use pkg2zip_core::error::{Error, Result};
use pkg2zip_core::output::Sink;
use pkg2zip_core::pkg::meta::META_INSTALL_DIRECTORY;
use pkg2zip_core::pkg::naming::{self, Naming};
use pkg2zip_core::pkg::{HEADER_TOTAL, Kind, MetaBlock, MetaInfo, PkgHeader, region, sfo};
use pkg2zip_core::plat::{DataFile, Ui};
use pkg2zip_core::psp::{self, PspContext, PspOptions};
use pkg2zip_core::vita::{self, VitaContext};
use pkg2zip_core::zrif::{rif_load, zrif_decode};

use args::{Config, Parsed};
use ui::ConsoleUi;

/// Código de salida que indica "funcionalidad aún no migrada".
pub const PENDING_EXIT_CODE: u8 = 3;

const HELP: &str = "\
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

Usage: pkg2zip [-x] [-c[N]] [-b] [-p] <file.pkg> [zRIF]
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args::parse(&args) {
        Ok(Parsed::Version) => {
            println!("pkg2zip {}", pkg2zip_core::VERSION);
            ExitCode::SUCCESS
        }
        Ok(Parsed::Help) => {
            println!("pkg2zip v{}", pkg2zip_core::VERSION);
            println!();
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        Ok(Parsed::Run(config)) => match run(&config) {
            Ok(Outcome::Done) => ExitCode::SUCCESS,
            Ok(Outcome::Pending(reason)) => {
                eprintln!("ERROR: {reason}");
                ExitCode::from(PENDING_EXIT_CODE)
            }
            Err(error) => {
                eprintln!("ERROR: {error}");
                ExitCode::from(1)
            }
        },
        Err(message) => {
            eprintln!("{message}");
            print!("{HELP}");
            ExitCode::from(1)
        }
    }
}

/// Resultado de una ejecución correcta.
#[derive(Debug, Clone, Copy)]
enum Outcome {
    Done,
    Pending(&'static str),
}

fn run(config: &Config) -> Result<Outcome> {
    let mut ui = ConsoleUi::new(!config.verbose);

    if config.verbose {
        println!("pkg2zip v{}", pkg2zip_core::VERSION);
        ui.info("[*] loading...");
    }

    let pkg = DataFile::open(Path::new(&config.pkg))?;
    let header_bytes = pkg.read_vec(0, HEADER_TOTAL)?;
    let header = PkgHeader::parse(&header_bytes)?;
    header.validate(pkg.len())?;

    let info = read_meta(&pkg, &header)?;
    let content_type = info
        .content_type
        .ok_or(Error::Corrupt("pkg has no content type"))?;
    let base_kind = Kind::from_content_type(content_type)?;

    match base_kind {
        Kind::VitaApp | Kind::VitaDlc | Kind::VitaPsm | Kind::VitaTheme | Kind::VitaPatch => {
            unpack_vita(config, &pkg, &header, &info, base_kind, &mut ui)
        }
        Kind::Psp | Kind::PspTheme | Kind::Psx | Kind::Ps3 => {
            unpack_psp_family(config, &pkg, &header, &info, base_kind, &mut ui)
        }
    }
}

fn read_meta(pkg: &DataFile, header: &PkgHeader) -> Result<MetaInfo> {
    let mut info = MetaInfo::default();
    let mut offset = u64::from(header.meta_offset);

    for _ in 0..header.meta_count {
        let block = MetaBlock::parse(&pkg.read_vec(offset, 16)?)?;
        info.apply(&block);
        if block.kind == META_INSTALL_DIRECTORY {
            let size =
                usize::try_from(block.size).map_err(|_| Error::TooShort("metadata block"))?;
            let data = pkg.read_vec(offset + 8, size)?;
            info.apply_install_directory(&data)?;
        }
        offset += u64::from(block.file_len());
    }

    Ok(info)
}

fn unpack_vita(
    config: &Config,
    pkg: &DataFile,
    header: &PkgHeader,
    info: &MetaInfo,
    base_kind: Kind,
    ui: &mut ConsoleUi,
) -> Result<Outcome> {
    let sfo_size = usize::try_from(info.sfo_size).map_err(|_| Error::TooShort("pkg file"))?;
    let sfo_bytes = pkg.read_vec(u64::from(info.sfo_offset), sfo_size)?;
    let sfo = sfo::parse(&sfo_bytes)?;
    let kind = base_kind.with_category(sfo.category());

    if kind == Kind::VitaPsm {
        return Ok(Outcome::Pending("PSM unpacking arrives in phase F4"));
    }

    let keys = derive_keys(header.key_type, header.hdr_type, &header.iv)?;
    let context = VitaContext {
        pkg,
        header,
        info,
        sfo: &sfo,
        kind,
        keys,
        zrif: config.zrif.as_deref(),
    };

    // El C valida el zRIF antes de calcular nombres o crear salida.
    let rif = vita::load_rif(&context)?;
    let root = vita::root_name(&context)?;

    if config.verbose {
        ui.info(unpacking_message(kind));
    }

    if config.listing {
        if !config.zipped {
            return Err(Error::Corrupt(
                "Listing option without creating zip is useless",
            ));
        }
        println!("{root}.zip");
        return Ok(Outcome::Done);
    }

    let name = if config.zipped {
        format!("{root}.zip")
    } else {
        root.clone()
    };

    if config.verbose {
        if config.zipped {
            ui.info(&format!("[*] creating '{name}' archive"));
        } else {
            ui.info(&format!("[*] unpacking '{name}' to directory"));
        }
    }

    let mut sink = Sink::create(&name, config.zipped)?;
    vita::unpack(
        &context,
        config.zipped,
        config.bgdl,
        rif.as_deref(),
        &mut sink,
        ui,
    )?;
    sink.finish()?;

    if config.verbose {
        if matches!(kind, Kind::VitaApp | Kind::VitaPatch)
            && let Some(min_version) = sfo.min_version.as_deref()
        {
            ui.info(&format!("[*] minimum fw version required: {min_version}"));
        }
        ui.info("[*] done!");
    }

    Ok(Outcome::Done)
}

fn unpack_psp_family(
    config: &Config,
    pkg: &DataFile,
    header: &PkgHeader,
    info: &MetaInfo,
    kind: Kind,
    ui: &mut ConsoleUi,
) -> Result<Outcome> {
    let keys = derive_keys(header.key_type, header.hdr_type, &header.iv)?;
    let mut category = String::new();
    let title;
    let id;
    let mut rif = None;

    match kind {
        Kind::Psp | Kind::Psx => {
            let parsed = psp::find_psp_sfo(pkg, header, info, &keys)?;
            category = parsed.category().to_owned();
            title = parsed.title().to_owned();
            let disc_id = psp::find_pbp_disc_id(pkg, header, info, &keys)?;
            id = if disc_id.is_empty() {
                header_serial(header)
            } else {
                disc_id
            };
            if kind == Kind::Psx
                && let Some(zrif) = config.zrif.as_deref()
            {
                rif = Some(if zrif.ends_with(".bin") {
                    rif_load(Path::new(zrif), 512)?
                } else {
                    zrif_decode(zrif, 512)?
                });
            }
        }
        Kind::PspTheme => {
            id = header_serial(header);
            title = String::new();
        }
        Kind::Ps3 => {
            id = header_serial(header);
            title = id.clone();
        }
        _ => {
            return Err(Error::Corrupt("unsupported kind for PSP unpacking"));
        }
    }

    let mut context = PspContext {
        pkg,
        header,
        info,
        keys,
        kind,
        title,
        id,
        category,
        rif,
    };

    if kind == Kind::PspTheme {
        let (offset, size) = psp::first_item_data(pkg, header, info, &context.keys)?;
        context.title = psp::get_psp_theme_title(&context, offset, size)?;
    }

    let id9: String = context.id.chars().take(9).collect();
    let naming = Naming {
        kind,
        title: &context.title,
        id: &id9,
        region: region(&id9),
        app_ver: "",
        content_type: info.content_type.unwrap_or(0),
        install_directory: info.install_directory.as_deref(),
        dlc_id: "",
        category: &context.category,
    };
    let root = naming::root_name(&naming);

    if config.verbose {
        ui.info(&unpacking_message_psp(kind, &naming));
    }

    if config.listing {
        if !config.zipped {
            return Err(Error::Corrupt(
                "Listing option without creating zip is useless",
            ));
        }
        println!("{root}.zip");
        return Ok(Outcome::Done);
    }

    let name = if config.zipped {
        format!("{root}.zip")
    } else {
        root.clone()
    };
    if config.verbose {
        if config.zipped {
            ui.info(&format!("[*] creating '{name}' archive"));
        } else {
            ui.info(&format!("[*] unpacking '{name}' to directory"));
        }
    }

    let mut sink = Sink::create(&name, config.zipped)?;
    psp::unpack(
        &context,
        PspOptions {
            pbp: config.pbp,
            ddlc: config.ddlc,
            cso: config.cso,
            zipped: config.zipped,
        },
        &mut sink,
        ui,
    )?;
    sink.finish()?;

    if config.verbose {
        ui.info("[*] done!");
    }

    Ok(Outcome::Done)
}

fn header_serial(header: &PkgHeader) -> String {
    let end = header
        .serial
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(header.serial.len());
    String::from_utf8_lossy(&header.serial[..end]).into_owned()
}

fn unpacking_message_psp(kind: Kind, naming: &Naming<'_>) -> String {
    match kind {
        Kind::Psp => format!("[*] unpacking {}", naming::psp_type_label(naming)),
        Kind::PspTheme => "[*] unpacking PSP Theme".to_owned(),
        Kind::Psx => "[*] unpacking PSX".to_owned(),
        Kind::Ps3 => "[*] unpacking PS3 PSX".to_owned(),
        _ => String::new(),
    }
}

fn unpacking_message(kind: Kind) -> &'static str {
    match kind {
        Kind::VitaDlc => "[*] unpacking Vita DLC",
        Kind::VitaPatch => "[*] unpacking Vita PATCH",
        Kind::VitaTheme => "[*] unpacking Vita theme",
        Kind::VitaPsm => "[*] unpacking Vita PSM",
        _ => "[*] unpacking Vita APP",
    }
}
