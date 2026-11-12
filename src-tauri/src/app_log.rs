//! A small, size-capped log file (`%LOCALAPPDATA%\aion2dps\app.log`) for
//! warnings and errors, plus overlay show results, so a problem in a tray app
//! with no console still leaves a trace. Over `CAP` the file is copied to
//! `app.log.1` and starts again, so at most about twice `CAP` is kept.
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const CAP: u64 = 512 * 1024;

pub struct RotatingLog {
    path: PathBuf,
    file: File,
    written: u64,
}

impl RotatingLog {
    pub fn open(path: &Path) -> io::Result<Self> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let written = file.metadata()?.len();
        let mut log = Self { path: path.to_path_buf(), file, written };
        if written >= CAP {
            log.rotate()?;
        }
        Ok(log)
    }

    /// Keep the old lines as `app.log.1` and empty the file. Copy and truncate,
    /// since Windows can't rename a file that is open.
    fn rotate(&mut self) -> io::Result<()> {
        let mut old = self.path.clone().into_os_string();
        old.push(".1");
        fs::copy(&self.path, PathBuf::from(old))?;
        self.file.set_len(0)?;
        self.written = 0;
        Ok(())
    }
}

impl Write for RotatingLog {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.written + buf.len() as u64 > CAP {
            let _ = self.rotate();
        }
        let n = self.file.write(buf)?;
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
