//! Tipo de PKG según `content_type` y `CATEGORY` del SFO.

use crate::error::{Error, Result};

/// Tipo de contenido de un PKG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Aplicación de PS Vita.
    VitaApp,
    /// DLC de PS Vita.
    VitaDlc,
    /// Parche de PS Vita (APP con `CATEGORY = "gp"`).
    VitaPatch,
    /// Título PSM.
    VitaPsm,
    /// Tema de PS Vita.
    VitaTheme,
    /// PSP (juego, Go, Mini, NeoGeo).
    Psp,
    /// Tema de PSP.
    PspTheme,
    /// PSX (PS1) para Adrenaline.
    Psx,
    /// PKG de PS3 que contiene un PSX.
    Ps3,
}

impl Kind {
    /// Convierte el `content_type` de la cabecera en un [`Kind`].
    ///
    /// # Errors
    ///
    /// [`Error::UnsupportedContentType`] si el valor no está soportado.
    pub fn from_content_type(content_type: u32) -> Result<Self> {
        match content_type {
            0x01 => Ok(Self::Ps3),
            0x06 => Ok(Self::Psx),
            0x07 | 0x0e | 0x0f | 0x10 => Ok(Self::Psp),
            0x09 => Ok(Self::PspTheme),
            0x15 => Ok(Self::VitaApp),
            0x16 => Ok(Self::VitaDlc),
            0x18 | 0x1d => Ok(Self::VitaPsm),
            0x1f => Ok(Self::VitaTheme),
            other => Err(Error::UnsupportedContentType(other)),
        }
    }

    /// Aplica la categoría del SFO: una APP con `CATEGORY = "gp"` es en realidad un PATCH.
    #[must_use]
    pub fn with_category(self, category: &str) -> Self {
        if self == Self::VitaApp && category == "gp" {
            Self::VitaPatch
        } else {
            self
        }
    }

    /// ¿Es un tipo de PS Vita (APP/PATCH/DLC/PSM/THEME)?
    #[must_use]
    pub fn is_vita(self) -> bool {
        matches!(
            self,
            Self::VitaApp | Self::VitaDlc | Self::VitaPatch | Self::VitaPsm | Self::VitaTheme
        )
    }

    /// ¿Necesita el bucle de items del contenedor PSP/PSX?
    #[must_use]
    pub fn is_psp_family(self) -> bool {
        matches!(self, Self::Psp | Self::PspTheme | Self::Psx | Self::Ps3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_content_types() {
        assert_eq!(Kind::from_content_type(0x15).expect("app"), Kind::VitaApp);
        assert_eq!(Kind::from_content_type(0x16).expect("dlc"), Kind::VitaDlc);
        assert_eq!(
            Kind::from_content_type(0x1f).expect("tema"),
            Kind::VitaTheme
        );
        assert_eq!(Kind::from_content_type(0x06).expect("psx"), Kind::Psx);
        assert_eq!(Kind::from_content_type(0x01).expect("ps3"), Kind::Ps3);
        assert_eq!(Kind::from_content_type(0x07).expect("psp"), Kind::Psp);
        assert_eq!(
            Kind::from_content_type(0x09).expect("tema psp"),
            Kind::PspTheme
        );
        assert_eq!(Kind::from_content_type(0x18).expect("psm"), Kind::VitaPsm);
        assert_eq!(Kind::from_content_type(0x1d).expect("psm"), Kind::VitaPsm);
    }

    #[test]
    fn rejects_unknown_content_type() {
        assert!(matches!(
            Kind::from_content_type(0x99),
            Err(Error::UnsupportedContentType(0x99))
        ));
    }

    #[test]
    fn category_gp_becomes_patch() {
        assert_eq!(Kind::VitaApp.with_category("gp"), Kind::VitaPatch);
        assert_eq!(Kind::VitaApp.with_category("gd"), Kind::VitaApp);
        assert_eq!(Kind::VitaDlc.with_category("gp"), Kind::VitaDlc);
    }
}
