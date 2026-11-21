//! Nombre del archivo de salida (el que imprime `-l`).
//!
//! Port de la construcción de `root` en `legacy/pkg2zip.c`.

use crate::pkg::Kind;

/// Datos necesarios para construir el nombre.
#[derive(Debug, Clone, Copy)]
pub struct Naming<'a> {
    /// Tipo de PKG (ya refinado con `CATEGORY`).
    pub kind: Kind,
    /// Título del SFO (o el del PSAR en PSP).
    pub title: &'a str,
    /// ID de 9 caracteres (`PCSE00000`).
    pub id: &'a str,
    /// Región (`USA`, `EUR`, ...).
    pub region: &'a str,
    /// `APP_VER` del SFO (para PATCH).
    pub app_ver: &'a str,
    /// `content_type` original (para distinguir PSP-Go/Mini/NeoGeo).
    pub content_type: u32,
    /// Directorio de instalación de DLC de PSP.
    pub install_directory: Option<&'a str>,
    /// Sufijo de DLC (`id2` en el C).
    pub dlc_id: &'a str,
    /// `CATEGORY` del SFO (para PSP-PCEngine).
    pub category: &'a str,
}

/// Construye el nombre raíz **sin** extensión.
///
/// El llamador añade `.zip` cuando corresponde.
#[must_use]
pub fn root_name(naming: &Naming<'_>) -> String {
    let title = naming.title;
    let id = naming.id;
    let region = naming.region;

    match naming.kind {
        Kind::Psp => {
            let type_str = if naming.content_type == 7 {
                if naming.category == "HG" {
                    "PSP-PCEngine"
                } else if naming.install_directory.is_some_and(|dir| !dir.is_empty()) {
                    "PSP-DLC"
                } else {
                    "PSP"
                }
            } else {
                match naming.content_type {
                    0x0e => "PSP-Go",
                    0x0f => "PSP-Mini",
                    _ => "PSP-NeoGeo",
                }
            };
            format!("{title} [{id}] [{type_str}]")
        }
        Kind::PspTheme => format!("{title} [{id}] [PSP-Theme]"),
        Kind::Psx | Kind::Ps3 => format!("{title} [{id}] [{region}] [PSX]"),
        Kind::VitaDlc => format!("{title} [{id}] [{region}] [DLC-{}]", naming.dlc_id),
        Kind::VitaPatch => format!("{title} [{id}] [{region}] [PATCH] [v{}]", naming.app_ver),
        Kind::VitaPsm => format!("{id} [{region}] [PSM]"),
        Kind::VitaApp | Kind::VitaTheme => format!("{title} [{id}] [{region}]"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naming(kind: Kind) -> Naming<'static> {
        Naming {
            kind,
            title: "Mi Juego",
            id: "PCSE00000",
            region: "USA",
            app_ver: "1.0",
            content_type: 0x15,
            install_directory: None,
            dlc_id: "0000000000000000",
            category: "gd",
        }
    }

    #[test]
    fn vita_names() {
        assert_eq!(
            root_name(&naming(Kind::VitaApp)),
            "Mi Juego [PCSE00000] [USA]"
        );
        assert_eq!(
            root_name(&naming(Kind::VitaPatch)),
            "Mi Juego [PCSE00000] [USA] [PATCH] [v1.0]"
        );
        assert_eq!(
            root_name(&naming(Kind::VitaDlc)),
            "Mi Juego [PCSE00000] [USA] [DLC-0000000000000000]"
        );
        assert_eq!(root_name(&naming(Kind::VitaPsm)), "PCSE00000 [USA] [PSM]");
        assert_eq!(
            root_name(&naming(Kind::VitaTheme)),
            "Mi Juego [PCSE00000] [USA]"
        );
    }

    #[test]
    fn psp_names() {
        let mut psp = naming(Kind::Psp);
        psp.content_type = 7;
        assert_eq!(root_name(&psp), "Mi Juego [PCSE00000] [PSP]");

        psp.install_directory = Some("DLCDIR");
        assert_eq!(root_name(&psp), "Mi Juego [PCSE00000] [PSP-DLC]");

        let mut mini = naming(Kind::Psp);
        mini.content_type = 0x0f;
        assert_eq!(root_name(&mini), "Mi Juego [PCSE00000] [PSP-Mini]");

        let mut go = naming(Kind::Psp);
        go.content_type = 0x0e;
        assert_eq!(root_name(&go), "Mi Juego [PCSE00000] [PSP-Go]");

        let mut theme = naming(Kind::PspTheme);
        theme.content_type = 9;
        assert_eq!(root_name(&theme), "Mi Juego [PCSE00000] [PSP-Theme]");

        assert_eq!(
            root_name(&naming(Kind::Psx)),
            "Mi Juego [PCSE00000] [USA] [PSX]"
        );
    }
}
