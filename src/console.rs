/// Attach or allocate a Windows console when the exe is built with the GUI
/// subsystem (same idea as Go `-H windowsgui`).
pub fn prepare_console(mode: &str) {
    #[cfg(windows)]
    windows_impl::prepare(mode);
    #[cfg(not(windows))]
    let _ = mode;
}

#[cfg(windows)]
mod windows_impl {
    use std::fs::{File, OpenOptions};
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;

    use windows::core::w;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Console::{
        AllocConsole, AttachConsole, GetConsoleMode, GetConsoleWindow, SetConsoleCP,
        SetConsoleMode, SetConsoleOutputCP, SetConsoleTitleW, SetStdHandle, ATTACH_PARENT_PROCESS,
        CONSOLE_MODE, DISABLE_NEWLINE_AUTO_RETURN, ENABLE_EXTENDED_FLAGS, ENABLE_PROCESSED_OUTPUT,
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
        enable_vt();
        unsafe {
            let _ = SetConsoleCP(UTF8);
            let _ = SetConsoleOutputCP(UTF8);
            if mode == "tui" {
                let _ = SetConsoleTitleW(w!("SSH tunnels"));
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

    fn or_mode(mode: CONSOLE_MODE, flag: CONSOLE_MODE) -> CONSOLE_MODE {
        CONSOLE_MODE(mode.0 | flag.0)
    }

    fn enable_vt() {
        if let Some(f) = CON_IN.get() {
            unsafe {
                let h = raw_handle(f);
                let mut mode = CONSOLE_MODE(0);
                if GetConsoleMode(h, &mut mode).is_ok() {
                    mode = or_mode(mode, ENABLE_VIRTUAL_TERMINAL_INPUT);
                    mode = or_mode(mode, ENABLE_EXTENDED_FLAGS);
                    mode = or_mode(mode, ENABLE_WINDOW_INPUT);
                    mode = CONSOLE_MODE(mode.0 & !ENABLE_QUICK_EDIT_MODE.0);
                    let _ = SetConsoleMode(h, mode);
                }
            }
        }
        if let Some(f) = CON_OUT.get() {
            unsafe {
                let h = raw_handle(f);
                let mut mode = CONSOLE_MODE(0);
                if GetConsoleMode(h, &mut mode).is_ok() {
                    mode = or_mode(mode, ENABLE_PROCESSED_OUTPUT);
                    mode = or_mode(mode, ENABLE_WRAP_AT_EOL_OUTPUT);
                    mode = or_mode(mode, ENABLE_VIRTUAL_TERMINAL_PROCESSING);
                    mode = or_mode(mode, DISABLE_NEWLINE_AUTO_RETURN);
                    let _ = SetConsoleMode(h, mode);
                }
            }
        }
    }
}
