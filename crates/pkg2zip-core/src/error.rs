//! Errores tipados del dominio.
//!
//! Los mensajes buscan ser compatibles con los del binario C (`legacy/`), porque los tests de
//! paridad comparan stdout/stderr. La capa CLI antepone `ERROR: ` una sola vez.

/// Resultado con el error del dominio.
pub type Result<T> = std::result::Result<T, Error>;

/// Error del dominio de `pkg2zip`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error de entrada/salida con el archivo o directorio indicado.
    #[error("{context}: {source}")]
    Io {
        /// Qué se estaba haciendo (p. ej. `"cannot open 'game.pkg'"`).
        context: String,
        /// Error de sistema original.
        #[source]
        source: std::io::Error,
    },

    /// El archivo no empieza con la firma PKG.
    #[error("not a pkg file")]
    NotAPkg,

    /// El archivo (o una estructura interna) es más corto de lo que declara.
    #[error("{0} is too short")]
    TooShort(&'static str),

    /// Datos internos incoherentes.
    #[error("{0}")]
    Corrupt(&'static str),

    /// `content_type` desconocido en la cabecera.
    #[error("unsupported content type 0x{0:x}")]
    UnsupportedContentType(u32),

    /// `key_type` desconocido en la cabecera.
    #[error("unsupported key type {0}")]
    UnsupportedKeyType(u8),

    /// Funcionalidad aún no migrada (F0).
    #[error("not implemented yet: {0}")]
    NotImplemented(&'static str),
}

impl Error {
    /// Construye un [`Error::Io`] con contexto legible.
    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}
