//! Consola: mensajes informativos, progreso y UTF-8 en Windows.

use std::io::{IsTerminal, Write};

use pkg2zip_core::plat::Ui;

/// Salida de la CLI.
#[derive(Debug)]
pub(crate) struct ConsoleUi {
    quiet: bool,
    tty: bool,
    progress_total: u64,
    next_percent: u32,
    #[cfg(windows)]
    old_codepage: Option<u32>,
}

impl ConsoleUi {
    /// Crea la UI y, en Windows, fija la consola a UTF-8 (como `sys_output_init`).
    #[must_use]
    pub(crate) fn new(quiet: bool) -> Self {
        Self {
            quiet,
            tty: std::io::stdout().is_terminal(),
            progress_total: 0,
            next_percent: 0,
            #[cfg(windows)]
            old_codepage: set_utf8_codepage(),
        }
    }
}

#[cfg(windows)]
fn set_utf8_codepage() -> Option<u32> {
    // SAFETY: GetConsoleOutputCP/SetConsoleOutputCP no reciben punteros y son seguras de llamar
    // desde un solo hilo durante el arranque de la CLI.
    #[allow(unsafe_code)]
    unsafe {
        use windows_sys::Win32::System::Console::{GetConsoleOutputCP, SetConsoleOutputCP};
        let previous = GetConsoleOutputCP();
        if SetConsoleOutputCP(65001) == 0 {
            None
        } else {
            Some(previous)
        }
    }
}

#[cfg(windows)]
impl Drop for ConsoleUi {
    fn drop(&mut self) {
        if let Some(previous) = self.old_codepage {
            // SAFETY: ver `set_utf8_codepage`.
            #[allow(unsafe_code)]
            unsafe {
                use windows_sys::Win32::System::Console::SetConsoleOutputCP;
                SetConsoleOutputCP(previous);
            }
        }
    }
}

impl Ui for ConsoleUi {
    fn info(&mut self, message: &str) {
        if !self.quiet {
            println!("{message}");
        }
    }

    fn info_raw(&mut self, message: &str) {
        if !self.quiet {
            print!("{message}");
            let _ = std::io::stdout().flush();
        }
    }

    fn progress_init(&mut self, total: u64) {
        self.progress_total = total;
        self.next_percent = 0;
    }

    fn progress(&mut self, position: u64) {
        if !self.tty || self.progress_total == 0 {
            return;
        }
        let percent =
            u32::try_from(position.saturating_mul(100) / self.progress_total).unwrap_or(u32::MAX);
        if percent >= self.next_percent {
            print!("[*] unpacking... {percent}%\r");
            let _ = std::io::stdout().flush();
            self.next_percent = percent.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_suppresses_info() {
        let mut ui = ConsoleUi::new(true);
        ui.info("no debería verse");
        ui.progress(10);
    }
}
