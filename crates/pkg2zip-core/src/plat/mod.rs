//! Utilidades de plataforma: IO posicional y abstracción de salida.

pub mod file;

pub use file::{DataFile, OutputFile, SilentUi, Ui, create_dir_all, is_dir};
