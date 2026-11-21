//! Detección de región a partir del ID de contenido.
//!
//! Port literal de `get_region` (`legacy/pkg2zip.c`). El ID real es `CONTENT_ID[7..16]`; aquí se
//! reciben los 4 primeros caracteres del ID (`PCSE`, `ULES`, ...).

/// Región asociada a un ID de 9 caracteres (o a su prefijo de 4).
#[must_use]
pub fn region(id: &str) -> &'static str {
    #[allow(clippy::match_same_arms)]
    let prefix = match id.get(..4) {
        Some(prefix) => prefix,
        None => return "UNK",
    };

    match prefix {
        // EUR
        "NPEE" | "NPEF" | "UCES" | "ULES" | "NPEG" | "NPEH" | "NPEX" | "NPEZ" | "PCSB" | "PCSF"
        | "NPOA" => "EUR",
        // ASA
        "NPHI" | "NPHJ" | "UCAS" | "ULAS" | "NPHG" | "NPHH" | "NPHZ" | "PCSD" | "PCSH" | "NPQA" => {
            "ASA"
        }
        // JPN
        "NPJI" | "NPJJ" | "UCJM" | "ULJM" | "UCJS" | "ULJS" | "UCJB" | "NPJG" | "NPJH" | "PCSC"
        | "PCSG" | "NPPA" => "JPN",
        // USA
        "NPUF" | "NPUI" | "NPUJ" | "UCUS" | "ULUS" | "NPUG" | "NPUH" | "NPUX" | "NPUZ" | "PCSA"
        | "PCSE" | "NPNA" => "USA",
        // KOR
        "UCKS" | "ULKS" => "KOR",
        // INT
        "PCSI" | "NPXS" => "INT",
        _ => "UNK",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_prefixes() {
        assert_eq!(region("PCSE00000"), "USA");
        assert_eq!(region("PCSB00000"), "EUR");
        assert_eq!(region("PCSG00000"), "JPN");
        assert_eq!(region("PCSH00000"), "ASA");
        assert_eq!(region("UCKS00000"), "KOR");
        assert_eq!(region("PCSI00000"), "INT");
        assert_eq!(region("XXXX00000"), "UNK");
        assert_eq!(region("PC"), "UNK");
        assert_eq!(region(""), "UNK");
    }
}
