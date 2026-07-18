use std::fs;
use std::path::PathBuf;

use tauri::{AppHandle, Manager};

pub fn data_dir(app: &AppHandle) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(configured) = std::env::var_os("VIVIDARIUM_DATA_DIR") {
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
    fs::create_dir_all(&path)?;
    Ok(path)
}
