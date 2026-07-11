use std::fs;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

pub fn data_dir(app: &AppHandle) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(configured) = std::env::var_os("PHYTOINDEX_DATA_DIR") {
        let path = PathBuf::from(configured);
        fs::create_dir_all(&path)?;
        return Ok(path);
    }
    if cfg!(debug_assertions) {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../data");
        fs::create_dir_all(&path)?;
        return Ok(path);
    }
    let path = app.path().app_data_dir()?;
    migrate_legacy_data(&path)?;
    fs::create_dir_all(&path)?;
    Ok(path)
}

fn migrate_legacy_data(destination: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if destination.join("phytoindex.db").exists() {
        return Ok(());
    }
    let Some(source) = legacy_data_dir() else {
        return Ok(());
    };
    if !source.is_dir() || source == destination {
        return Ok(());
    }
    fs::create_dir_all(destination)?;
    let database = source.join("phytoindex.db");
    if database.is_file() {
        fs::copy(database, destination.join("phytoindex.db"))?;
    }
    let thumbnails = source.join("thumbnails");
    if thumbnails.is_dir() {
        copy_directory(&thumbnails, &destination.join("thumbnails"))?;
    }
    Ok(())
}

fn legacy_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        return std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|path| path.join("PhytoIndex"));
    }
    #[cfg(target_os = "macos")]
    {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|path| path.join("Library/Application Support/PhytoIndex"));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
            return Some(PathBuf::from(path).join("PhytoIndex"));
        }
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|path| path.join(".local/share/PhytoIndex"));
    }
    #[allow(unreachable_code)]
    None
}

fn copy_directory(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
