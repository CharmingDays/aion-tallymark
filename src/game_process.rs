//! Detect whether the AION 2 client is running by listing process names only.
//!
//! Uses a Toolhelp snapshot of image names. It never opens a handle to another
//! process, never reads its memory, and never sends it anything. The name comes
//! from the Steam install path `...\AION2\Aion2\Binaries\Win64\AION2.exe`.

/// Confirmed executable name of the AION 2 game client.
pub const AION2_EXE: &str = "AION2.exe";

/// True when a process whose image name is [`AION2_EXE`] is running.
pub fn aion2_running() -> bool {
    #[cfg(windows)]
    {
        process_named(AION2_EXE)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
fn process_named(exe: &str) -> bool {
    use std::mem::size_of;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            cntUsage: 0,
            th32ProcessID: 0,
            th32DefaultHeapID: 0,
            th32ModuleID: 0,
            cntThreads: 0,
            th32ParentProcessID: 0,
            pcPriClassBase: 0,
            dwFlags: 0,
            szExeFile: [0; 260],
        };
        let mut found = false;
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                let name = String::from_utf16_lossy(
                    &entry.szExeFile[..entry
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szExeFile.len())],
                );
                if name.eq_ignore_ascii_case(exe) {
                    found = true;
                    break;
                }
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_client_exe_name_is_aion2() {
        assert_eq!(AION2_EXE, "AION2.exe");
        assert!(AION2_EXE.ends_with(".exe"));
    }
}
