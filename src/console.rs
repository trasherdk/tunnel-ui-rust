/// Attach or allocate a Windows console when the exe is built with the GUI
/// subsystem (same idea as Go `-H windowsgui`).
pub fn prepare_console(mode: &str) {
    #[cfg(windows)]
    windows_impl::prepare(mode);
    #[cfg(not(windows))]
    let _ = mode;
}

/// Crossterm raw mode enables `ENABLE_VIRTUAL_TERMINAL_INPUT`, which makes
/// Windows deliver arrows/Esc as CSI bytes. Crossterm's Windows reader uses
/// `ReadConsoleInput` and `VK_*` codes, so those keys are dropped. Clear VT
/// input after `ratatui::init()`.
pub fn apply_tui_console_mode() {
    #[cfg(windows)]
    windows_impl::apply_tui_mode();
}

#[cfg(windows)]
mod windows_impl {
    use std::fs::{File, OpenOptions};
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;

    use windows::core::w;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Console::{
        AllocConsole, AttachConsole, GetConsoleMode, GetConsoleWindow, GetStdHandle, SetConsoleCP,
        SetConsoleMode, SetConsoleOutputCP, SetConsoleTitleW, SetStdHandle, ATTACH_PARENT_PROCESS,
        CONSOLE_MODE, DISABLE_NEWLINE_AUTO_RETURN, ENABLE_ECHO_INPUT, ENABLE_EXTENDED_FLAGS,
        ENABLE_LINE_INPUT, ENABLE_MOUSE_INPUT, ENABLE_PROCESSED_INPUT, ENABLE_PROCESSED_OUTPUT,
        ENABLE_QUICK_EDIT_MODE, ENABLE_VIRTUAL_TERMINAL_INPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
        ENABLE_WINDOW_INPUT, ENABLE_WRAP_AT_EOL_OUTPUT, STD_ERROR_HANDLE, STD_INPUT_HANDLE,
        STD_OUTPUT_HANDLE,
    };
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

    const UTF8: u32 = 65001;

    static CON_IN: OnceLock<File> = OnceLock::new();
    static CON_OUT: OnceLock<File> = OnceLock::new();

    pub fn prepare(mode: &str) {
        set_aumid();
        if mode == "supervisor" {
            return;
        }
        unsafe {
            if GetConsoleWindow().is_invalid() {
                if AttachConsole(ATTACH_PARENT_PROCESS).is_err() {
                    let _ = AllocConsole();
                }
            }
        }
        bind_stdio();
        enable_output_vt();
        unsafe {
            let _ = SetConsoleCP(UTF8);
            let _ = SetConsoleOutputCP(UTF8);
            if mode == "tui" {
                let _ = SetConsoleTitleW(w!("SSH tunnels"));
            }
        }
    }

    pub fn apply_tui_mode() {
        unsafe {
            if let Ok(h) = GetStdHandle(STD_INPUT_HANDLE) {
                if !h.is_invalid() {
                    let mut mode = CONSOLE_MODE(0);
                    if GetConsoleMode(h, &mut mode).is_ok() {
                        mode = CONSOLE_MODE(
                            (mode.0
                                | ENABLE_EXTENDED_FLAGS.0
                                | ENABLE_WINDOW_INPUT.0
                                | ENABLE_MOUSE_INPUT.0)
                                & !(ENABLE_VIRTUAL_TERMINAL_INPUT.0
                                    | ENABLE_QUICK_EDIT_MODE.0
                                    | ENABLE_LINE_INPUT.0
                                    | ENABLE_ECHO_INPUT.0
                                    | ENABLE_PROCESSED_INPUT.0),
                        );
                        let _ = SetConsoleMode(h, mode);
                    }
                }
            }
            if let Ok(h) = GetStdHandle(STD_OUTPUT_HANDLE) {
                if !h.is_invalid() {
                    let mut mode = CONSOLE_MODE(0);
                    if GetConsoleMode(h, &mut mode).is_ok() {
                        mode = CONSOLE_MODE(
                            (mode.0
                                | ENABLE_PROCESSED_OUTPUT.0
                                | ENABLE_VIRTUAL_TERMINAL_PROCESSING.0
                                | DISABLE_NEWLINE_AUTO_RETURN.0)
                                & !ENABLE_WRAP_AT_EOL_OUTPUT.0,
                        );
                        let _ = SetConsoleMode(h, mode);
                    }
                }
            }
        }
    }

    fn set_aumid() {
        unsafe {
            let _ = SetCurrentProcessExplicitAppUserModelID(w!("dk.fumlersoft.tunnel-ui"));
        }
    }

    fn raw_handle(f: &File) -> HANDLE {
        HANDLE(f.as_raw_handle() as _)
    }

    fn bind_stdio() {
        let Ok(stdin) = OpenOptions::new().read(true).write(true).open("CONIN$") else {
            return;
        };
        let Ok(stdout) = OpenOptions::new().read(true).write(true).open("CONOUT$") else {
            return;
        };
        unsafe {
            let _ = SetStdHandle(STD_INPUT_HANDLE, raw_handle(&stdin));
            let _ = SetStdHandle(STD_OUTPUT_HANDLE, raw_handle(&stdout));
            let _ = SetStdHandle(STD_ERROR_HANDLE, raw_handle(&stdout));
        }
        let _ = CON_IN.set(stdin);
        let _ = CON_OUT.set(stdout);
    }

    fn enable_output_vt() {
        unsafe {
            if let Ok(h) = GetStdHandle(STD_OUTPUT_HANDLE) {
                if h.is_invalid() {
                    return;
                }
                let mut mode = CONSOLE_MODE(0);
                if GetConsoleMode(h, &mut mode).is_ok() {
                    mode = CONSOLE_MODE(
                        (mode.0
                            | ENABLE_PROCESSED_OUTPUT.0
                            | ENABLE_VIRTUAL_TERMINAL_PROCESSING.0
                            | DISABLE_NEWLINE_AUTO_RETURN.0)
                            & !ENABLE_WRAP_AT_EOL_OUTPUT.0,
                    );
                    let _ = SetConsoleMode(h, mode);
                }
            }
        }
    }
}
