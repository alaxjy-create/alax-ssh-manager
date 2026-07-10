use crate::config::AppPaths;
use crate::ssh::TerminalCommand;
use crate::transfer::TransferHandle;
use crate::tunnel::TunnelHandle;
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::mpsc::Sender;

pub struct AppState {
    pub paths: AppPaths,
    pub terminals: Mutex<HashMap<String, Sender<TerminalCommand>>>,
    pub transfers: Mutex<HashMap<String, TransferHandle>>,
    pub tunnels: Mutex<HashMap<String, TunnelHandle>>,
}

impl AppState {
    pub fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            terminals: Mutex::new(HashMap::new()),
            transfers: Mutex::new(HashMap::new()),
            tunnels: Mutex::new(HashMap::new()),
        }
    }
}
