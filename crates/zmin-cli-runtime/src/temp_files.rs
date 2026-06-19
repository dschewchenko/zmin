use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{CliError, Result};

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);
const CONTENT_ADDRESSED_FILE_BUF_CAPACITY: usize = 256 * 1024;

pub fn write_content_addressed_file(path: &Path, bytes: &[u8]) -> Result<()> {
    match fs::metadata(path) {
        Ok(metadata) => {
            if metadata.len() == bytes.len() as u64 && fs::read(path)? == bytes {
                return Ok(());
            }
            return Err(CliError::Io(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} already exists with different contents", path.display()),
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(CliError::Io(error)),
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = unique_temp_sibling(path);
    write_unique_temp_file(&tmp, bytes)?;
    match fs::hard_link(&tmp, path) {
        Ok(()) => {
            let _ = fs::remove_file(&tmp);
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&tmp);
            if fs::metadata(path).is_ok_and(|metadata| metadata.len() == bytes.len() as u64)
                && fs::read(path).is_ok_and(|existing| existing == bytes)
            {
                Ok(())
            } else {
                Err(CliError::Io(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{} already exists with different contents", path.display()),
                )))
            }
        }
        Err(error) => {
            let _ = fs::remove_file(&tmp);
            Err(CliError::Io(error))
        }
    }
}

fn write_unique_temp_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let mut file = io::BufWriter::with_capacity(CONTENT_ADDRESSED_FILE_BUF_CAPACITY, file);
    file.write_all(bytes)?;
    file.flush()?;
    Ok(())
}

pub fn unique_temp_sibling(path: &Path) -> PathBuf {
    let mut value = std::ffi::OsString::from(path.as_os_str());
    value.push(format!(
        ".tmp-{}-{}",
        std::process::id(),
        TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    PathBuf::from(value)
}
