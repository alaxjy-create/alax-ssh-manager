use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub database_path: PathBuf,
    pub log_dir: PathBuf,
}

impl AppPaths {
    pub fn resolve(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let data_dir = app.path().app_data_dir()?;
        let database_path = data_dir.join("alax-ssh-manager.sqlite3");
        let log_dir = data_dir.join("logs");

        Ok(Self {
            data_dir,
            database_path,
            log_dir,
        })
    }
}
