use crate::error::Result;
use std::{
    fs::{self, File},
    io::{self, Write},
    path::Path,
};

/// Instance-owned persistence operations; defaults keep real files and OS errors.
pub trait Persistence: Send + Sync {
    fn before_receipt_update(&self) -> Result<()> {
        Ok(())
    }

    fn write(&self, file: &mut File, bytes: &[u8]) -> io::Result<()> {
        file.write_all(bytes)
    }

    fn sync(&self, file: &File) -> io::Result<()> {
        file.sync_all()
    }

    fn link(&self, original: &Path, target: &Path) -> io::Result<()> {
        fs::hard_link(original, target)
    }

    fn unlink(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }
}

pub struct NativePersistence;
impl Persistence for NativePersistence {}
