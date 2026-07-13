use crate::{credentials, db, elevated_sftp, logs, sftp, ssh, state::AppState, transfer, tunnel};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{AppHandle, State};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    pub mount: String,
    pub used: u64,
    pub total: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInfo {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStats {
    pub cpu_usage: f64,
    pub memory_used: u64,
    pub memory_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub disks: Vec<DiskInfo>,
    pub networks: Vec<NetworkInfo>,
    pub temperature: Option<f64>,
    pub uptime: f64,
    pub load_avg_1: f64,
    pub load_avg_5: f64,
    pub load_avg_15: f64,
}

#[tauri::command]
pub async fn get_server_stats(
    state: State<'_, AppState>,
    server_id: String,
) -> Result<ServerStats, String> {
    let server = db::get_server_connection(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;

    let script = r#"
echo "===STATS==="
head -1 /proc/stat
sleep 0.5
head -1 /proc/stat
free -b | grep -E "^(Mem|Swap):"
df -B1 2>/dev/null | tail -n +2
echo "===NET==="
cat /proc/net/dev 2>/dev/null | tail -n +3
echo "===TEMP==="
(cat /sys/class/thermal/thermal_zone*/temp 2>/dev/null || echo "N/A") | head -1
echo "===UPTIME==="
cat /proc/uptime
echo "===LOAD==="
cat /proc/loadavg
"#;

    let output = ssh::run_command(&server, script).await?;
    let all_lines: Vec<&str> = output.lines().collect();

    // Find section markers
    let mut stats_pos = None;
    let mut net_pos = None;
    let mut temp_pos = None;
    let mut uptime_pos = None;
    let mut load_pos = None;
    for (i, line) in all_lines.iter().enumerate() {
        if *line == "===STATS===" {
            stats_pos = Some(i);
        }
        if *line == "===NET===" {
            net_pos = Some(i);
        }
        if *line == "===TEMP===" {
            temp_pos = Some(i);
        }
        if *line == "===UPTIME===" {
            uptime_pos = Some(i);
        }
        if *line == "===LOAD===" {
            load_pos = Some(i);
        }
    }

    // Lines between ===STATS=== and ===NET===
    let body = section_lines(&all_lines, stats_pos, net_pos);

    // CPU: first two lines
    let cpu_usage = if body.len() >= 2 {
        let cpu1 = parse_cpu_line(body[0]);
        let cpu2 = parse_cpu_line(body[1]);
        if cpu1.1 > 0.0 && cpu2.0 > cpu1.0 && cpu2.1 >= cpu1.1 {
            let total_delta = cpu2.0 - cpu1.0;
            let idle_delta = cpu2.1 - cpu1.1;
            ((1.0 - idle_delta / total_delta) * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        }
    } else {
        0.0
    };

    // Memory (Mem:/Swap:)
    let mut memory_used = 0u64;
    let mut memory_total = 0u64;
    let mut swap_used = 0u64;
    let mut swap_total = 0u64;
    for line in &body {
        if line.starts_with("Mem:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                memory_total = parts[1].parse().unwrap_or(0);
                memory_used = parts[2].parse().unwrap_or(0);
            }
        } else if line.starts_with("Swap:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                swap_total = parts[1].parse().unwrap_or(0);
                swap_used = parts[2].parse().unwrap_or(0);
            }
        }
    }

    // Disks (lines starting with / after memory lines)
    let mut disks = Vec::new();
    let mut in_df = false;
    for line in &body {
        if line.starts_with('/') {
            in_df = true;
        }
        if in_df && line.starts_with('/') {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                disks.push(DiskInfo {
                    mount: parts.last().unwrap_or(&"").to_string(),
                    used: parts[2].parse().unwrap_or(0),
                    total: parts[1].parse().unwrap_or(0),
                });
            }
        }
    }

    // Network
    let mut networks = Vec::new();
    let net_lines = section_lines(&all_lines, net_pos, temp_pos);
    for line in &net_lines {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 10 {
            let name = parts[0].trim_end_matches(':');
            let rx: u64 = parts[1].parse().unwrap_or(0);
            let tx: u64 = parts[9].parse().unwrap_or(0);
            networks.push(NetworkInfo {
                name: name.to_string(),
                rx_bytes: rx,
                tx_bytes: tx,
            });
        }
    }

    // Temperature
    let temp_line = line_after_marker(&all_lines, temp_pos)
        .unwrap_or("N/A")
        .trim();
    let temperature = if temp_line != "N/A" {
        temp_line.parse::<f64>().ok().map(|v| v / 1000.0)
    } else {
        None
    };

    // Uptime
    let uptime_val = line_after_marker(&all_lines, uptime_pos)
        .unwrap_or("0")
        .split_whitespace()
        .next()
        .unwrap_or("0")
        .parse()
        .unwrap_or(0.0);

    // Load average
    let load_parts: Vec<&str> = line_after_marker(&all_lines, load_pos)
        .unwrap_or("0 0 0")
        .split_whitespace()
        .collect();
    let load_avg_1 = load_parts.first().unwrap_or(&"0").parse().unwrap_or(0.0);
    let load_avg_5 = load_parts.get(1).unwrap_or(&"0").parse().unwrap_or(0.0);
    let load_avg_15 = load_parts.get(2).unwrap_or(&"0").parse().unwrap_or(0.0);

    Ok(ServerStats {
        cpu_usage,
        memory_used,
        memory_total,
        swap_used,
        swap_total,
        disks,
        networks,
        temperature,
        uptime: uptime_val,
        load_avg_1,
        load_avg_5,
        load_avg_15,
    })
}

fn parse_cpu_line(line: &str) -> (f64, f64) {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 5 || parts[0] != "cpu" {
        return (0.0, 0.0);
    }
    let total: f64 = parts[1..]
        .iter()
        .map(|s| s.parse::<f64>().unwrap_or(0.0))
        .sum();
    let idle = parts[4].parse::<f64>().unwrap_or(0.0);
    (total, idle)
}

fn section_lines<'a>(
    lines: &'a [&'a str],
    start: Option<usize>,
    end: Option<usize>,
) -> Vec<&'a str> {
    let Some(start) = start else {
        return Vec::new();
    };
    let end = end.unwrap_or(lines.len()).min(lines.len());
    if start + 1 >= end {
        return Vec::new();
    }
    lines[start + 1..end]
        .iter()
        .copied()
        .filter(|line| !line.is_empty())
        .collect()
}

fn line_after_marker<'a>(lines: &'a [&'a str], marker: Option<usize>) -> Option<&'a str> {
    marker.and_then(|index| lines.get(index + 1).copied())
}

async fn ensure_remote_path_exists(
    server: &db::ServerConnectionConfig,
    path: &str,
    message: &str,
) -> Result<(), String> {
    match elevated_sftp::remote_path_exists(server, path).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(format!("{message}: {path}")),
        Err(error) => Err(format!("{message}; verification failed: {error}")),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub database_ready: bool,
    pub credential_store: String,
    pub log_directory: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSession {
    pub id: String,
    pub server_id: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogPath {
    pub path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePreview {
    pub path: String,
    pub name: String,
    pub mime: String,
    pub preview_kind: String,
    pub size: u64,
    pub truncated: bool,
    pub data_url: Option<String>,
    pub text: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyStatus {
    pub status: String,
    pub algorithm: String,
    pub fingerprint: String,
    pub trusted_fingerprint: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTextFile {
    pub path: String,
    pub text: String,
    pub size: u64,
    pub sha256: String,
}

#[tauri::command]
pub fn initialize_database(state: State<AppState>) -> Result<(), String> {
    db::initialize(&state.paths.database_path).map_err(|error| error.to_string())?;
    logs::append_event(&state.paths.log_dir, "info", "db", "SQLite initialized")
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_app_status(state: State<AppState>) -> Result<AppStatus, String> {
    Ok(AppStatus {
        database_ready: state.paths.database_path.exists(),
        credential_store: crate::credentials::store_name().to_string(),
        log_directory: state.paths.log_dir.to_string_lossy().to_string(),
    })
}

#[tauri::command]
pub fn list_groups(state: State<AppState>) -> Result<Vec<db::ServerGroup>, String> {
    db::list_groups(&state.paths.database_path).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_servers(state: State<AppState>) -> Result<Vec<db::ServerProfile>, String> {
    db::list_servers(&state.paths.database_path).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_host_key_status(
    state: State<'_, AppState>,
    server_id: String,
) -> Result<HostKeyStatus, String> {
    let server = db::get_server_connection(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    let current = ssh::probe_host_key(&server).await?;
    let trusted = db::get_trusted_host_key(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    let trusted_fingerprint = trusted.as_ref().map(|key| key.fingerprint.clone());
    let status = match trusted.as_ref() {
        None => "unknown",
        Some(key) if key.fingerprint == current.fingerprint => "trusted",
        Some(_) => "changed",
    };
    Ok(HostKeyStatus {
        status: status.to_string(),
        algorithm: current.algorithm,
        fingerprint: current.fingerprint,
        trusted_fingerprint,
    })
}

#[tauri::command]
pub async fn trust_host_key(
    state: State<'_, AppState>,
    server_id: String,
    fingerprint: String,
) -> Result<db::TrustedHostKey, String> {
    let server = db::get_server_connection(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    let current = ssh::probe_host_key(&server).await?;
    if current.fingerprint != fingerprint {
        return Err("主机密钥在确认过程中发生变化，已拒绝保存。请重新检查服务器身份。".to_string());
    }
    let trusted = db::trust_host_key(
        &state.paths.database_path,
        &server_id,
        &current.algorithm,
        &current.fingerprint,
    )
    .map_err(|error| error.to_string())?;
    logs::append_event(&state.paths.log_dir, "info", "ssh", "SSH host key trusted")
        .map_err(|error| error.to_string())?;
    Ok(trusted)
}

#[tauri::command]
pub fn clear_trusted_host_key(state: State<AppState>, server_id: String) -> Result<(), String> {
    db::clear_trusted_host_key(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    logs::append_event(
        &state.paths.log_dir,
        "info",
        "ssh",
        "SSH host key trust cleared",
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_log_directory(state: State<AppState>) -> Result<String, String> {
    Ok(state.paths.log_dir.to_string_lossy().to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub credential_store: String,
    pub log_directory: String,
    pub database_directory: String,
}

#[tauri::command]
pub fn get_app_info(state: State<AppState>) -> Result<AppInfo, String> {
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        credential_store: crate::credentials::store_name().to_string(),
        log_directory: state.paths.log_dir.to_string_lossy().to_string(),
        database_directory: state.paths.data_dir.to_string_lossy().to_string(),
    })
}

#[tauri::command]
pub fn read_logs(
    state: State<AppState>,
    max_lines: Option<usize>,
) -> Result<Vec<logs::LogEntry>, String> {
    logs::read_recent_logs(&state.paths.log_dir, max_lines.unwrap_or(200).min(5_000))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn open_log_directory(state: State<AppState>) -> Result<String, String> {
    std::fs::create_dir_all(&state.paths.log_dir).map_err(|error| error.to_string())?;
    open_path(&state.paths.log_dir)?;
    Ok(state.paths.log_dir.to_string_lossy().to_string())
}

#[tauri::command]
pub fn create_group(
    state: State<AppState>,
    input: db::GroupInput,
) -> Result<db::ServerGroup, String> {
    let input = normalize_group_input(input);
    validate_group_input(&state.paths.database_path, &input, false)?;
    let group =
        db::create_group(&state.paths.database_path, input).map_err(|error| error.to_string())?;
    logs::append_event(&state.paths.log_dir, "info", "group", "Group created")
        .map_err(|error| error.to_string())?;
    Ok(group)
}

#[tauri::command]
pub fn update_group(
    state: State<AppState>,
    input: db::GroupInput,
) -> Result<db::ServerGroup, String> {
    let input = normalize_group_input(input);
    validate_group_input(&state.paths.database_path, &input, true)?;
    let group =
        db::update_group(&state.paths.database_path, input).map_err(|error| error.to_string())?;
    logs::append_event(&state.paths.log_dir, "info", "group", "Group updated")
        .map_err(|error| error.to_string())?;
    Ok(group)
}

#[tauri::command]
pub fn delete_group(state: State<AppState>, group_id: String) -> Result<(), String> {
    db::delete_group(&state.paths.database_path, &group_id).map_err(|error| error.to_string())?;
    logs::append_event(&state.paths.log_dir, "info", "group", "Group deleted")
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn create_server(
    state: State<AppState>,
    input: db::ServerInput,
) -> Result<db::ServerProfile, String> {
    let input = normalize_server_input(input);
    validate_server_input(&input)?;
    validate_create_secrets(&input)?;
    let server_id = input
        .id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let (credential_ref, private_key_ref) = save_input_secrets(&server_id, &input)?;
    let input = db::ServerInput {
        id: Some(server_id),
        ..input
    };
    let server = match db::create_server(
        &state.paths.database_path,
        input,
        credential_ref.clone(),
        private_key_ref.clone(),
    ) {
        Ok(server) => server,
        Err(error) => {
            delete_secret_refs([credential_ref.as_deref(), private_key_ref.as_deref()]);
            return Err(error.to_string());
        }
    };
    logs::append_event(&state.paths.log_dir, "info", "server", "Server created")
        .map_err(|error| error.to_string())?;
    Ok(server)
}

#[tauri::command]
pub fn update_server(
    state: State<AppState>,
    input: db::ServerInput,
) -> Result<db::ServerProfile, String> {
    let input = normalize_server_input(input);
    validate_server_input(&input)?;
    let server_id = input
        .id
        .clone()
        .ok_or_else(|| "Missing server id".to_string())?;
    let previous = db::get_server_secret_state(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    validate_update_secrets(&state.paths.database_path, &server_id, &input)?;
    let (credential_ref, private_key_ref) = save_input_secrets(&server_id, &input)?;
    let server = match db::update_server(
        &state.paths.database_path,
        input,
        credential_ref.clone(),
        private_key_ref.clone(),
    ) {
        Ok(server) => server,
        Err(error) => {
            delete_secret_refs([credential_ref.as_deref(), private_key_ref.as_deref()]);
            return Err(error.to_string());
        }
    };
    let current = db::get_server_secret_state(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    if previous.credential_ref != current.credential_ref {
        delete_secret_refs([previous.credential_ref.as_deref()]);
    }
    if previous.private_key_ref != current.private_key_ref {
        delete_secret_refs([previous.private_key_ref.as_deref()]);
    }
    logs::append_event(&state.paths.log_dir, "info", "server", "Server updated")
        .map_err(|error| error.to_string())?;
    Ok(server)
}

#[tauri::command]
pub fn duplicate_server(
    state: State<AppState>,
    server_id: String,
) -> Result<db::ServerProfile, String> {
    let profile = db::get_server(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    let connection = db::get_server_connection(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    let new_id = uuid::Uuid::new_v4().to_string();

    let credential_ref = connection
        .credential_ref
        .as_deref()
        .map(|reference| copy_secret(reference, &new_id, "credential"))
        .transpose()?;
    let private_key_ref = match connection.private_key_ref.as_deref() {
        Some(reference) => match copy_secret(reference, &new_id, "private-key") {
            Ok(reference) => Some(reference),
            Err(error) => {
                delete_secret_refs([credential_ref.as_deref()]);
                return Err(error);
            }
        },
        None => None,
    };

    let input = db::ServerInput {
        id: Some(new_id),
        name: format!("{} 副本", profile.name),
        host: profile.host,
        port: profile.port,
        username: profile.username,
        auth_type: profile.auth_type,
        password: None,
        use_empty_password: false,
        private_key_path: connection.private_key_path,
        private_key_content: None,
        passphrase: None,
        group_id: profile.group_id,
        tags: profile.tags,
        note: profile.note,
    };

    let duplicated = match db::create_server(
        &state.paths.database_path,
        input,
        credential_ref.clone(),
        private_key_ref.clone(),
    ) {
        Ok(server) => server,
        Err(error) => {
            delete_secret_refs([credential_ref.as_deref(), private_key_ref.as_deref()]);
            return Err(error.to_string());
        }
    };
    logs::append_event(&state.paths.log_dir, "info", "server", "Server duplicated")
        .map_err(|error| error.to_string())?;
    Ok(duplicated)
}

#[tauri::command]
pub fn delete_server(state: State<AppState>, server_id: String) -> Result<(), String> {
    {
        let mut tunnels = state
            .tunnels
            .lock()
            .map_err(|_| "Tunnel state lock failed".to_string())?;
        let ids: Vec<String> = tunnels
            .iter()
            .filter(|(_, handle)| handle.server_id == server_id)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            if let Some(handle) = tunnels.remove(&id) {
                handle.stop();
            }
        }
    }
    {
        let mut transfers = state
            .transfers
            .lock()
            .map_err(|_| "Transfer state lock failed".to_string())?;
        let ids: Vec<String> = transfers
            .iter()
            .filter(|(_, handle)| handle.input.server_id == server_id)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            if let Some(handle) = transfers.remove(&id) {
                handle.cancel.store(true, Ordering::Relaxed);
            }
        }
    }
    let (credential_ref, private_key_ref) =
        db::delete_server(&state.paths.database_path, &server_id)
            .map_err(|error| error.to_string())?;
    if let Some(reference) = credential_ref {
        let _ = credentials::delete_secret(&reference);
    }
    if let Some(reference) = private_key_ref {
        let _ = credentials::delete_secret(&reference);
    }
    logs::append_event(&state.paths.log_dir, "info", "server", "Server deleted")
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn list_tunnel_rules(
    state: State<AppState>,
    server_id: String,
) -> Result<Vec<db::TunnelRule>, String> {
    db::list_tunnel_rules(&state.paths.database_path, &server_id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_tunnel_rule(
    state: State<AppState>,
    mut input: db::TunnelRuleInput,
) -> Result<db::TunnelRule, String> {
    input.name = input.name.trim().to_string();
    input.local_host = input.local_host.trim().to_string();
    input.remote_host = input.remote_host.trim().to_string();
    validate_tunnel_rule(&input)?;
    let _ = db::get_server(&state.paths.database_path, &input.server_id)
        .map_err(|error| error.to_string())?;
    if let Some(id) = input.id.as_ref() {
        let tunnels = state
            .tunnels
            .lock()
            .map_err(|_| "Tunnel state lock failed".to_string())?;
        if tunnels.get(id).is_some_and(|handle| handle.is_alive()) {
            return Err("请先停止隧道再修改规则。".to_string());
        }
    }
    let rule = db::save_tunnel_rule(&state.paths.database_path, input)
        .map_err(|error| error.to_string())?;
    logs::append_event(&state.paths.log_dir, "info", "tunnel", "Tunnel rule saved")
        .map_err(|error| error.to_string())?;
    Ok(rule)
}

#[tauri::command]
pub async fn start_tunnel(
    state: State<'_, AppState>,
    tunnel_id: String,
) -> Result<db::TunnelRule, String> {
    {
        let mut tunnels = state
            .tunnels
            .lock()
            .map_err(|_| "Tunnel state lock failed".to_string())?;
        tunnels.retain(|_, handle| handle.is_alive());
        if tunnels.contains_key(&tunnel_id) {
            return db::get_tunnel_rule(&state.paths.database_path, &tunnel_id)
                .map_err(|error| error.to_string());
        }
        if tunnels.len() >= 32 {
            return Err("最多同时运行 32 条 SSH 隧道。".to_string());
        }
    }
    let rule = db::get_tunnel_rule(&state.paths.database_path, &tunnel_id)
        .map_err(|error| error.to_string())?;
    let server = db::get_server_connection(&state.paths.database_path, &rule.server_id)
        .map_err(|error| error.to_string())?;
    let handle = tunnel::start(server, rule.clone()).await?;
    state
        .tunnels
        .lock()
        .map_err(|_| "Tunnel state lock failed".to_string())?
        .insert(tunnel_id, handle);
    logs::append_event(
        &state.paths.log_dir,
        "info",
        "tunnel",
        "Local SSH tunnel started",
    )
    .map_err(|error| error.to_string())?;
    Ok(rule)
}

#[tauri::command]
pub fn stop_tunnel(state: State<AppState>, tunnel_id: String) -> Result<(), String> {
    if let Some(handle) = state
        .tunnels
        .lock()
        .map_err(|_| "Tunnel state lock failed".to_string())?
        .remove(&tunnel_id)
    {
        handle.stop();
    }
    logs::append_event(
        &state.paths.log_dir,
        "info",
        "tunnel",
        "Local SSH tunnel stopped",
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn list_active_tunnels(state: State<AppState>) -> Result<Vec<String>, String> {
    let mut tunnels = state
        .tunnels
        .lock()
        .map_err(|_| "Tunnel state lock failed".to_string())?;
    tunnels.retain(|_, handle| handle.is_alive());
    Ok(tunnels.keys().cloned().collect())
}

#[tauri::command]
pub fn delete_tunnel_rule(state: State<AppState>, tunnel_id: String) -> Result<(), String> {
    if let Some(handle) = state
        .tunnels
        .lock()
        .map_err(|_| "Tunnel state lock failed".to_string())?
        .remove(&tunnel_id)
    {
        handle.stop();
    }
    db::delete_tunnel_rule(&state.paths.database_path, &tunnel_id)
        .map_err(|error| error.to_string())?;
    logs::append_event(
        &state.paths.log_dir,
        "info",
        "tunnel",
        "Tunnel rule deleted",
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn test_connection(
    state: State<'_, AppState>,
    server_id: String,
) -> Result<String, String> {
    let server = db::get_server_connection(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    match ssh::connect(&server).await {
        Ok(handle) => {
            ssh::disconnect(&handle).await;
            db::set_server_status(&state.paths.database_path, &server_id, "available")
                .map_err(|error| error.to_string())?;
            logs::append_event(
                &state.paths.log_dir,
                "info",
                "ssh",
                "SSH authentication succeeded",
            )
            .map_err(|error| error.to_string())?;
            Ok(format!(
                "SSH authentication succeeded: {}@{}:{}",
                server.username, server.host, server.port
            ))
        }
        Err(error) => {
            let _ = db::set_server_status(&state.paths.database_path, &server_id, "error");
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn open_terminal(
    app: AppHandle,
    state: State<'_, AppState>,
    server_id: String,
    cols: Option<u32>,
    rows: Option<u32>,
) -> Result<TerminalSession, String> {
    if state
        .terminals
        .lock()
        .map_err(|_| "Terminal state lock failed".to_string())?
        .len()
        >= 32
    {
        return Err("最多同时打开 32 个终端会话。".to_string());
    }
    let server = db::get_server_connection(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    let (session, sender) = ssh::spawn_pty(
        app,
        server,
        cols.unwrap_or(100).clamp(20, 1_000),
        rows.unwrap_or(30).clamp(5, 500),
    );
    state
        .terminals
        .lock()
        .map_err(|_| "Terminal state lock failed".to_string())?
        .insert(session.id.clone(), sender);
    logs::append_event(
        &state.paths.log_dir,
        "info",
        "ssh",
        "Terminal session opened",
    )
    .map_err(|error| error.to_string())?;
    Ok(session)
}

#[tauri::command]
pub async fn terminal_write(
    state: State<'_, AppState>,
    session_id: String,
    data: String,
) -> Result<(), String> {
    if data.len() > 1024 * 1024 {
        return Err("单次终端输入不能超过 1 MB。".to_string());
    }
    let sender = {
        let terminals = state
            .terminals
            .lock()
            .map_err(|_| "Terminal state lock failed".to_string())?;
        terminals
            .get(&session_id)
            .cloned()
            .ok_or_else(|| "Terminal session not found".to_string())?
    };
    sender
        .send(ssh::TerminalCommand::Write(data))
        .await
        .map_err(|error| format!("Unable to send terminal input: {error}"))
}

#[tauri::command]
pub async fn terminal_resize(
    state: State<'_, AppState>,
    session_id: String,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    let sender = {
        let terminals = state
            .terminals
            .lock()
            .map_err(|_| "Terminal state lock failed".to_string())?;
        terminals
            .get(&session_id)
            .cloned()
            .ok_or_else(|| "Terminal session not found".to_string())?
    };
    sender
        .send(ssh::TerminalCommand::Resize {
            cols: cols.clamp(20, 1_000),
            rows: rows.clamp(5, 500),
        })
        .await
        .map_err(|error| format!("Unable to resize terminal: {error}"))
}

#[tauri::command]
pub async fn close_terminal(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    let sender = state
        .terminals
        .lock()
        .map_err(|_| "Terminal state lock failed".to_string())?
        .remove(&session_id);
    if let Some(sender) = sender {
        ssh::close_session(&sender).await;
    }
    logs::append_event(
        &state.paths.log_dir,
        "info",
        "ssh",
        "Terminal session closed",
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn sftp_read_dir(
    state: State<'_, AppState>,
    server_id: String,
    path: String,
) -> Result<Vec<sftp::RemoteFileEntry>, String> {
    validate_remote_path(&path, false)?;
    let server = db::get_server_connection(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    let handle = ssh::connect(&server).await?;
    let entries = match sftp::read_dir(&handle, &path).await {
        Ok(entries) => Ok(entries),
        Err(error) if elevated_sftp::should_try_elevation(&error) => {
            let _ = logs::append_event(
                &state.paths.log_dir,
                "warn",
                "sftp",
                &format!("Read directory with SFTP failed, retrying with sudo: {path}"),
            );
            elevated_sftp::read_dir(&server, &handle, &path).await
        }
        Err(error) => Err(error),
    };
    ssh::disconnect(&handle).await;
    let entries = entries?;
    logs::append_event(
        &state.paths.log_dir,
        "info",
        "sftp",
        "Remote directory read",
    )
    .map_err(|error| error.to_string())?;
    Ok(entries)
}

#[tauri::command]
pub async fn sftp_create_dir(
    state: State<'_, AppState>,
    server_id: String,
    path: String,
) -> Result<(), String> {
    validate_remote_path(&path, false)?;
    let server = db::get_server_connection(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    let handle = ssh::connect(&server).await?;
    match sftp::create_dir(&handle, &path).await {
        Ok(()) => {
            if matches!(
                elevated_sftp::remote_path_exists(&server, &path).await,
                Ok(false)
            ) {
                let _ = logs::append_event(
                    &state.paths.log_dir,
                    "warn",
                    "sftp",
                    "Create directory reported success but path is missing; retrying with sudo",
                );
                elevated_sftp::create_dir(&server, &path).await?;
            }
        }
        Err(error) if elevated_sftp::should_try_elevation(&error) => {
            elevated_sftp::create_dir(&server, &path).await?;
        }
        Err(error) => return Err(error),
    }
    ensure_remote_path_exists(&server, &path, "Remote directory was not created").await?;
    logs::append_event(
        &state.paths.log_dir,
        "info",
        "sftp",
        "Remote directory created",
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn sftp_create_file(
    state: State<'_, AppState>,
    server_id: String,
    path: String,
) -> Result<(), String> {
    validate_remote_path(&path, false)?;
    let server = db::get_server_connection(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    let handle = ssh::connect(&server).await?;
    match sftp::create_file(&handle, &path).await {
        Ok(()) => {
            if matches!(
                elevated_sftp::remote_path_exists(&server, &path).await,
                Ok(false)
            ) {
                let _ = logs::append_event(
                    &state.paths.log_dir,
                    "warn",
                    "sftp",
                    "Create file reported success but path is missing; retrying with sudo",
                );
                elevated_sftp::create_file(&server, &path).await?;
            }
        }
        Err(error) if elevated_sftp::should_try_elevation(&error) => {
            elevated_sftp::create_file(&server, &path).await?;
        }
        Err(error) => return Err(error),
    }
    ensure_remote_path_exists(&server, &path, "Remote file was not created").await?;
    logs::append_event(&state.paths.log_dir, "info", "sftp", "Remote file created")
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn sftp_delete(
    state: State<'_, AppState>,
    server_id: String,
    paths: Vec<String>,
) -> Result<(), String> {
    if paths.is_empty() {
        return Err("No remote paths selected".to_string());
    }
    for path in &paths {
        validate_remote_path(path, true)?;
    }
    let server = db::get_server_connection(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    let _ = logs::append_event(
        &state.paths.log_dir,
        "info",
        "sftp",
        &format!("Delete request: {} paths", paths.len()),
    );
    let handle = ssh::connect(&server).await?;
    let result: Result<(), String> = async {
        for (i, path) in paths.iter().enumerate() {
            let _ = logs::append_event(&state.paths.log_dir, "info", "sftp", &format!("Deleting [{i}] {path}"));
            match sftp::delete_path_recursive(&handle, path).await {
                Ok(()) => {
                    match elevated_sftp::remote_path_exists(&server, path).await {
                        Ok(true) => {
                            let _ = logs::append_event(
                                &state.paths.log_dir,
                                "warn",
                                "sftp",
                                &format!("Delete [{i}] {path}: SFTP reported success but path still exists; retrying with sudo"),
                            );
                            elevated_sftp::delete_path(&server, path).await.map_err(|e| {
                                let _ = logs::append_event(&state.paths.log_dir, "error", "sftp", &format!("Elevated delete [{i}] {path}: {e}"));
                                e
                            })?;
                        }
                        Ok(false) => {}
                        Err(error) => {
                            let _ = logs::append_event(&state.paths.log_dir, "warn", "sftp", &format!("Delete [{i}] {path}: unable to verify removal: {error}"));
                        }
                    }
                }
                Err(error) if elevated_sftp::should_try_elevation(&error) => {
                    elevated_sftp::delete_path(&server, path).await.map_err(|e| {
                        let _ = logs::append_event(&state.paths.log_dir, "error", "sftp", &format!("Delete [{i}] {path}: {e}"));
                        e
                    })?;
                }
                Err(error) => {
                    let _ = logs::append_event(&state.paths.log_dir, "error", "sftp", &format!("Delete [{i}] {path}: {error}"));
                    return Err(error);
                }
            }
            match elevated_sftp::remote_path_exists(&server, path).await {
                Ok(false) => {}
                Ok(true) => {
                    let message = format!("Delete [{i}] {path}: remote path still exists after delete");
                    let _ = logs::append_event(&state.paths.log_dir, "error", "sftp", &message);
                    return Err(message);
                }
                Err(error) => {
                    let _ = logs::append_event(&state.paths.log_dir, "warn", "sftp", &format!("Delete [{i}] {path}: final verification failed: {error}"));
                }
            }
        }
        Ok(())
    }
    .await;
    ssh::disconnect(&handle).await;
    if let Err(ref error) = result {
        let _ = logs::append_event(
            &state.paths.log_dir,
            "error",
            "sftp",
            &format!("Delete batch failed: {error}"),
        );
    }
    result?;
    logs::append_event(&state.paths.log_dir, "info", "sftp", "Remote path deleted")
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn sftp_rename(
    state: State<'_, AppState>,
    server_id: String,
    from: String,
    to: String,
) -> Result<(), String> {
    validate_remote_path(&from, true)?;
    validate_remote_path(&to, false)?;
    let server = db::get_server_connection(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    let handle = ssh::connect(&server).await?;
    match sftp::rename_path(&handle, &from, &to).await {
        Ok(()) => {
            if matches!(
                elevated_sftp::remote_path_exists(&server, &to).await,
                Ok(false)
            ) {
                let _ = logs::append_event(
                    &state.paths.log_dir,
                    "warn",
                    "sftp",
                    "Rename reported success but destination is missing; retrying with sudo",
                );
                elevated_sftp::rename_path(&server, &from, &to).await?;
            }
        }
        Err(error) if elevated_sftp::should_try_elevation(&error) => {
            elevated_sftp::rename_path(&server, &from, &to).await?;
        }
        Err(error) => return Err(error),
    }
    ensure_remote_path_exists(&server, &to, "Remote rename destination does not exist").await?;
    if from != to
        && matches!(
            elevated_sftp::remote_path_exists(&server, &from).await,
            Ok(true)
        )
    {
        return Err("Remote rename source still exists after operation".to_string());
    }
    logs::append_event(&state.paths.log_dir, "info", "sftp", "Remote path renamed")
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn sftp_upload_file(
    state: State<'_, AppState>,
    server_id: String,
    local_path: String,
    remote_path: String,
) -> Result<(), String> {
    validate_remote_path(&remote_path, false)?;
    let server = db::get_server_connection(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    let handle = ssh::connect(&server).await?;
    match sftp::upload_file(&handle, &local_path, &remote_path).await {
        Ok(()) => {
            if matches!(
                elevated_sftp::remote_path_exists(&server, &remote_path).await,
                Ok(false)
            ) {
                let _ = logs::append_event(
                    &state.paths.log_dir,
                    "warn",
                    "sftp",
                    "Upload reported success but remote file is missing; retrying with sudo",
                );
                let cancel = AtomicBool::new(false);
                elevated_sftp::upload_file_with_progress(
                    &server,
                    &local_path,
                    &remote_path,
                    &cancel,
                    |_, _, _| {},
                )
                .await?;
            }
        }
        Err(error) if elevated_sftp::should_try_elevation(&error) => {
            let cancel = AtomicBool::new(false);
            elevated_sftp::upload_file_with_progress(
                &server,
                &local_path,
                &remote_path,
                &cancel,
                |_, _, _| {},
            )
            .await?;
        }
        Err(error) => return Err(error),
    }
    ensure_remote_path_exists(&server, &remote_path, "Remote file was not uploaded").await?;
    logs::append_event(&state.paths.log_dir, "info", "sftp", "File uploaded")
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn sftp_download_file(
    state: State<'_, AppState>,
    server_id: String,
    remote_path: String,
    local_path: String,
) -> Result<(), String> {
    validate_remote_path(&remote_path, false)?;
    let server = db::get_server_connection(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    let handle = ssh::connect(&server).await?;
    match sftp::download_file(&handle, &remote_path, &local_path).await {
        Ok(()) => {}
        Err(error) if elevated_sftp::should_try_elevation(&error) => {
            let cancel = AtomicBool::new(false);
            elevated_sftp::download_file_with_progress(
                &server,
                &remote_path,
                &local_path,
                &cancel,
                |_, _, _| {},
            )
            .await?;
        }
        Err(error) => return Err(error),
    }
    logs::append_event(&state.paths.log_dir, "info", "sftp", "File downloaded")
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn sftp_preview_file(
    state: State<'_, AppState>,
    server_id: String,
    remote_path: String,
) -> Result<RemotePreview, String> {
    validate_remote_path(&remote_path, false)?;
    let server = db::get_server_connection(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    let mime = mime_for_path(&remote_path);
    let preview_kind = preview_kind_for_mime(&mime, &remote_path);
    let max_bytes = match preview_kind.as_str() {
        "image" | "video" | "audio" | "pdf" => 8 * 1024 * 1024,
        "text" => 512 * 1024,
        _ => 0,
    };
    let name = file_name_from_path(&remote_path);

    if max_bytes == 0 {
        return Ok(RemotePreview {
            path: remote_path,
            name,
            mime,
            preview_kind,
            size: 0,
            truncated: false,
            data_url: None,
            text: None,
            message: Some(
                "该文件类型暂不支持内嵌预览，可使用右键菜单下载或用系统打开。".to_string(),
            ),
        });
    }

    let handle = ssh::connect(&server).await?;
    let read_result = sftp::read_file_prefix(&handle, &remote_path, max_bytes).await;
    let (encoded, size, truncated) = match read_result {
        Ok((bytes, size, truncated)) => (encode_base64(&bytes), size, truncated),
        Err(error) if elevated_sftp::should_try_elevation(&error) => {
            elevated_sftp::read_file_base64(&server, &remote_path, max_bytes).await?
        }
        Err(error) => return Err(error),
    };

    let (data_url, text, message) = match preview_kind.as_str() {
        "text" => {
            let bytes = decode_base64_lossy(&encoded);
            let text = String::from_utf8_lossy(&bytes).to_string();
            (
                None,
                Some(text),
                truncated.then(|| "文件较大，仅显示前 512 KB。".to_string()),
            )
        }
        "image" | "video" | "audio" | "pdf" => {
            if truncated {
                (
                    None,
                    None,
                    Some("文件超过 8 MB，已跳过内嵌预览，可下载或用系统打开。".to_string()),
                )
            } else {
                (Some(format!("data:{mime};base64,{encoded}")), None, None)
            }
        }
        _ => (None, None, Some("该文件类型暂不支持内嵌预览。".to_string())),
    };

    logs::append_event(
        &state.paths.log_dir,
        "info",
        "sftp",
        "Remote file preview read",
    )
    .map_err(|error| error.to_string())?;
    Ok(RemotePreview {
        path: remote_path,
        name,
        mime,
        preview_kind,
        size,
        truncated,
        data_url,
        text,
        message,
    })
}

#[tauri::command]
pub async fn sftp_read_text_file(
    state: State<'_, AppState>,
    server_id: String,
    remote_path: String,
) -> Result<RemoteTextFile, String> {
    validate_remote_path(&remote_path, false)?;
    const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;
    let server = db::get_server_connection(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    let bytes = read_remote_bytes_for_edit(&server, &remote_path).await?;
    if bytes.len() > MAX_TEXT_BYTES {
        return Err("文本编辑器只支持不超过 2 MB 的文件。".to_string());
    }
    let size = bytes.len() as u64;
    let text = String::from_utf8(bytes.clone())
        .map_err(|_| "该文件不是有效的 UTF-8 文本，已拒绝以文本方式编辑。".to_string())?;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    Ok(RemoteTextFile {
        path: remote_path,
        text,
        size,
        sha256,
    })
}

#[tauri::command]
pub async fn sftp_write_text_file(
    state: State<'_, AppState>,
    server_id: String,
    remote_path: String,
    text: String,
    expected_sha256: String,
) -> Result<String, String> {
    validate_remote_path(&remote_path, false)?;
    let bytes = text.as_bytes();
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("文本编辑器只支持不超过 2 MB 的文件。".to_string());
    }
    let server = db::get_server_connection(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    let current = read_remote_bytes_for_edit(&server, &remote_path).await?;
    let current_sha256 = format!("{:x}", Sha256::digest(&current));
    if !expected_sha256.is_empty() && current_sha256 != expected_sha256 {
        return Err(
            "远程文件在编辑期间已被其他程序修改。为避免覆盖，保存已取消；请重新加载后再编辑。"
                .to_string(),
        );
    }

    let handle = ssh::connect(&server).await?;
    match sftp::write_file_atomic(&handle, &remote_path, bytes).await {
        Ok(()) => {}
        Err(error) if elevated_sftp::should_try_elevation(&error) => {
            let local_temp =
                std::env::temp_dir().join(format!("alax-edit-{}.tmp", uuid::Uuid::new_v4()));
            tokio::fs::write(&local_temp, bytes)
                .await
                .map_err(|write_error| write_error.to_string())?;
            let cancel = AtomicBool::new(false);
            let result = elevated_sftp::upload_file_with_progress(
                &server,
                &local_temp.to_string_lossy(),
                &remote_path,
                &cancel,
                |_, _, _| {},
            )
            .await;
            let _ = tokio::fs::remove_file(&local_temp).await;
            result?;
        }
        Err(error) => return Err(error),
    }
    ensure_remote_path_exists(&server, &remote_path, "Remote text file was not saved").await?;
    logs::append_event(
        &state.paths.log_dir,
        "info",
        "sftp",
        "Remote text file saved",
    )
    .map_err(|error| error.to_string())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[tauri::command]
pub async fn sftp_set_permissions(
    state: State<'_, AppState>,
    server_id: String,
    paths: Vec<String>,
    mode: String,
    recursive: bool,
) -> Result<(), String> {
    if paths.is_empty() {
        return Err("No remote paths selected".to_string());
    }
    for path in &paths {
        validate_remote_path(path, false)?;
    }
    validate_permission_mode(&mode)?;
    let server = db::get_server_connection(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    let recursive_flag = if recursive { "-R " } else { "" };
    let mut command = format!("chmod {recursive_flag}{mode} --");
    for path in &paths {
        command.push(' ');
        command.push_str(&shell_quote(path));
    }
    match ssh::run_command(&server, &command).await {
        Ok(_) => {}
        Err(error) if elevated_sftp::should_try_elevation(&error) => {
            elevated_sftp::set_permissions(&server, &paths, &mode, recursive).await?;
        }
        Err(error) => return Err(error),
    }
    logs::append_event(
        &state.paths.log_dir,
        "info",
        "sftp",
        "Remote permissions changed",
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn sftp_checksum(
    state: State<'_, AppState>,
    server_id: String,
    remote_path: String,
    algorithm: String,
) -> Result<String, String> {
    validate_remote_path(&remote_path, false)?;
    let (program, expected_len) = match algorithm.as_str() {
        "sha256" => ("sha256sum", 64),
        "sha1" => ("sha1sum", 40),
        "md5" => ("md5sum", 32),
        _ => return Err("Unsupported checksum algorithm".to_string()),
    };
    let server = db::get_server_connection(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    let command = format!("{program} -- {}", shell_quote(&remote_path));
    let output = match ssh::run_command(&server, &command).await {
        Ok(output) => output,
        Err(error) if elevated_sftp::should_try_elevation(&error) => {
            elevated_sftp::checksum(&server, &remote_path, program).await?
        }
        Err(error) => return Err(error),
    };
    let checksum = output
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if checksum.len() != expected_len
        || !checksum
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err("Remote server returned an invalid checksum".to_string());
    }
    Ok(checksum)
}

#[tauri::command]
pub async fn sftp_compress_paths(
    state: State<'_, AppState>,
    server_id: String,
    paths: Vec<String>,
    destination: String,
) -> Result<(), String> {
    validate_remote_path(&destination, false)?;
    for path in &paths {
        validate_remote_path(path, false)?;
    }
    let server = db::get_server_connection(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    if paths.is_empty() {
        return Err("No remote paths selected for compression".to_string());
    }
    let command = build_tar_command(&paths, &destination)?;
    match ssh::run_command(&server, &command).await {
        Ok(_) => {}
        Err(error) if elevated_sftp::should_try_elevation(&error) => {
            elevated_sftp::compress_paths(&server, &paths, &destination).await?;
        }
        Err(error) => return Err(error),
    }
    ensure_remote_path_exists(&server, &destination, "Remote archive was not created").await?;
    logs::append_event(
        &state.paths.log_dir,
        "info",
        "sftp",
        "Remote paths compressed",
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn sftp_open_remote_file(
    state: State<'_, AppState>,
    server_id: String,
    remote_path: String,
) -> Result<String, String> {
    validate_remote_path(&remote_path, false)?;
    let server = db::get_server_connection(&state.paths.database_path, &server_id)
        .map_err(|error| error.to_string())?;
    let temp_path = std::env::temp_dir()
        .join("alax-ssh-manager-open")
        .join(format!(
            "{}-{}",
            uuid::Uuid::new_v4(),
            file_name_from_path(&remote_path)
        ));
    if let Some(parent) = temp_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let local_path = temp_path.to_string_lossy().to_string();
    let handle = ssh::connect(&server).await?;
    match sftp::download_file(&handle, &remote_path, &local_path).await {
        Ok(()) => {}
        Err(error) if elevated_sftp::should_try_elevation(&error) => {
            let cancel = AtomicBool::new(false);
            elevated_sftp::download_file_with_progress(
                &server,
                &remote_path,
                &local_path,
                &cancel,
                |_, _, _| {},
            )
            .await?;
        }
        Err(error) => return Err(error),
    }
    open_file_with_system(&temp_path)?;
    logs::append_event(
        &state.paths.log_dir,
        "info",
        "sftp",
        "Remote file opened with system",
    )
    .map_err(|error| error.to_string())?;
    Ok(local_path)
}

#[tauri::command]
pub fn pick_upload_file() -> Result<Option<DialogPath>, String> {
    Ok(rfd::FileDialog::new().pick_file().map(|path| DialogPath {
        path: path.to_string_lossy().to_string(),
    }))
}

#[tauri::command]
pub fn pick_upload_directory() -> Result<Option<DialogPath>, String> {
    Ok(rfd::FileDialog::new().pick_folder().map(|path| DialogPath {
        path: path.to_string_lossy().to_string(),
    }))
}

#[tauri::command]
pub fn pick_download_path(default_file_name: String) -> Result<Option<DialogPath>, String> {
    Ok(rfd::FileDialog::new()
        .set_file_name(default_file_name)
        .save_file()
        .map(|path| DialogPath {
            path: path.to_string_lossy().to_string(),
        }))
}

#[tauri::command]
pub fn pick_download_directory() -> Result<Option<DialogPath>, String> {
    Ok(rfd::FileDialog::new().pick_folder().map(|path| DialogPath {
        path: path.to_string_lossy().to_string(),
    }))
}

#[tauri::command]
pub async fn transfer_start(
    app: AppHandle,
    state: State<'_, AppState>,
    input: transfer::TransferInput,
) -> Result<transfer::TransferTask, String> {
    validate_remote_path(&input.remote_path, false)?;
    if !matches!(input.transfer_type.as_str(), "upload" | "download") {
        return Err("Unsupported transfer type".to_string());
    }
    if !matches!(input.entry_kind.as_str(), "file" | "directory") {
        return Err("Unsupported transfer entry kind".to_string());
    }
    if input.transfer_type == "upload" {
        let metadata = std::fs::metadata(&input.local_path)
            .map_err(|error| format!("Unable to inspect local upload path: {error}"))?;
        if (input.entry_kind == "directory") != metadata.is_dir() {
            return Err("Local upload type does not match the selected path".to_string());
        }
    }
    if state
        .transfers
        .lock()
        .map_err(|_| "Transfer state lock failed".to_string())?
        .values()
        .filter(|handle| handle.active.load(Ordering::Relaxed))
        .count()
        >= 64
    {
        return Err("最多同时保留 64 个活动传输任务。".to_string());
    }
    let server = db::get_server_connection(&state.paths.database_path, &input.server_id)
        .map_err(|error| error.to_string())?;
    let task = transfer::new_task(input.clone());
    let cancel = Arc::new(AtomicBool::new(false));
    let active = Arc::new(AtomicBool::new(true));
    state
        .transfers
        .lock()
        .map_err(|_| "Transfer state lock failed".to_string())?
        .insert(
            task.id.clone(),
            transfer::TransferHandle {
                input: input.clone(),
                cancel: cancel.clone(),
                active: active.clone(),
            },
        );
    transfer::spawn_transfer(app, task.id.clone(), server, input, cancel, active);
    logs::append_event(&state.paths.log_dir, "info", "transfer", "Transfer started")
        .map_err(|error| error.to_string())?;
    Ok(task)
}

#[tauri::command]
pub fn transfer_cancel(state: State<AppState>, task_id: String) -> Result<(), String> {
    if let Some(handle) = state
        .transfers
        .lock()
        .map_err(|_| "Transfer state lock failed".to_string())?
        .get(&task_id)
    {
        handle.cancel.store(true, Ordering::Relaxed);
    }
    logs::append_event(
        &state.paths.log_dir,
        "info",
        "transfer",
        "Transfer cancelled",
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn transfer_retry(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
) -> Result<(), String> {
    let input = {
        let transfers = state
            .transfers
            .lock()
            .map_err(|_| "Transfer state lock failed".to_string())?;
        let handle = transfers
            .get(&task_id)
            .ok_or_else(|| "Transfer task not found".to_string())?;
        if handle.active.load(Ordering::Relaxed) {
            return Err("Transfer task is already running".to_string());
        }
        handle.input.clone()
    };
    let server = db::get_server_connection(&state.paths.database_path, &input.server_id)
        .map_err(|error| error.to_string())?;
    let cancel = Arc::new(AtomicBool::new(false));
    let active = Arc::new(AtomicBool::new(true));
    {
        let mut transfers = state
            .transfers
            .lock()
            .map_err(|_| "Transfer state lock failed".to_string())?;
        if transfers
            .values()
            .filter(|handle| handle.active.load(Ordering::Relaxed))
            .count()
            >= 64
        {
            return Err("最多同时保留 64 个活动传输任务。".to_string());
        }
        if transfers
            .get(&task_id)
            .is_some_and(|handle| handle.active.load(Ordering::Relaxed))
        {
            return Err("Transfer task is already running".to_string());
        }
        transfers.insert(
            task_id.clone(),
            transfer::TransferHandle {
                input: input.clone(),
                cancel: cancel.clone(),
                active: active.clone(),
            },
        );
    }
    transfer::spawn_transfer(app, task_id, server, input, cancel, active);
    logs::append_event(&state.paths.log_dir, "info", "transfer", "Transfer retried")
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn normalize_server_input(mut input: db::ServerInput) -> db::ServerInput {
    input.name = input.name.trim().to_string();
    input.host = input.host.trim().trim_matches(['[', ']']).to_string();
    input.username = input.username.trim().to_string();
    input.note = input.note.trim().to_string();
    input.tags = input
        .tags
        .into_iter()
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .collect();
    input.private_key_path = input.private_key_path.take().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });
    input
}

fn normalize_group_input(mut input: db::GroupInput) -> db::GroupInput {
    input.name = input.name.trim().to_string();
    input.parent_id = input.parent_id.and_then(|id| {
        let id = id.trim().to_string();
        (!id.is_empty()).then_some(id)
    });
    input
}

fn validate_group_input(
    database_path: &std::path::Path,
    input: &db::GroupInput,
    require_id: bool,
) -> Result<(), String> {
    validate_text_field("分组名称", &input.name, 128, false)?;
    if require_id && input.id.as_deref().is_none_or(str::is_empty) {
        return Err("Missing group id".to_string());
    }

    let Some(parent_id) = input.parent_id.as_deref() else {
        return Ok(());
    };
    let groups = db::list_groups(database_path).map_err(|error| error.to_string())?;
    let mut current = Some(parent_id);
    for _ in 0..=groups.len() {
        let Some(id) = current else {
            return Ok(());
        };
        if input.id.as_deref() == Some(id) {
            return Err("分组不能移动到自身或其子分组中。".to_string());
        }
        let group = groups
            .iter()
            .find(|group| group.id == id)
            .ok_or_else(|| "Parent group not found".to_string())?;
        current = group.parent_id.as_deref();
    }
    Err("分组层级中存在循环引用。".to_string())
}

fn validate_server_input(input: &db::ServerInput) -> Result<(), String> {
    validate_text_field("服务器名称", &input.name, 128, false)?;
    validate_text_field("Host", &input.host, 253, false)?;
    validate_text_field("用户名", &input.username, 128, false)?;
    if input.host.chars().any(char::is_whitespace) {
        return Err("Host 不能包含空白字符。".to_string());
    }
    if !(1..=65_535).contains(&input.port) {
        return Err("SSH 端口必须在 1 到 65535 之间。".to_string());
    }
    if !matches!(
        input.auth_type.as_str(),
        "password" | "private_key" | "private_key_with_passphrase"
    ) {
        return Err("不支持的认证方式。".to_string());
    }
    if input.note.chars().count() > 4_096 {
        return Err("备注不能超过 4096 个字符。".to_string());
    }
    if input.tags.len() > 64
        || input
            .tags
            .iter()
            .any(|tag| tag.chars().count() > 64 || tag.chars().any(char::is_control))
    {
        return Err("标签数量或长度超出限制。".to_string());
    }
    Ok(())
}

fn validate_text_field(
    label: &str,
    value: &str,
    max_len: usize,
    allow_empty: bool,
) -> Result<(), String> {
    if !allow_empty && value.is_empty() {
        return Err(format!("{label}不能为空。"));
    }
    if value.chars().count() > max_len {
        return Err(format!("{label}不能超过 {max_len} 个字符。"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{label}不能包含控制字符。"));
    }
    Ok(())
}

fn open_path(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("explorer");
        command.arg(path);
        command
    };

    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(path);
        command
    };

    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(path);
        command
    };

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("无法打开目录：{error}"))
}

fn open_file_with_system(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("rundll32.exe");
        command.arg("shell32.dll,OpenAs_RunDLL");
        command.arg(path);
        command
    };

    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(path);
        command
    };

    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(path);
        command
    };

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Unable to open file: {error}"))
}

fn build_tar_command(paths: &[String], destination: &str) -> Result<String, String> {
    if paths.is_empty() {
        return Err("No remote paths selected for compression".to_string());
    }
    let parent = remote_parent(destination);
    if paths.iter().any(|path| remote_parent(path) != parent) {
        return Err("All compressed paths must be in the destination directory".to_string());
    }
    let mut command = format!(
        "sh -c {} sh {} {}",
        shell_quote("cd \"$1\" && shift && tar -czf \"$1\" -- \"$@\""),
        shell_quote(&parent),
        shell_quote(destination)
    );
    for path in paths {
        command.push(' ');
        command.push_str(&shell_quote(&remote_name(path)));
    }
    Ok(command)
}

fn remote_parent(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) => "/".to_string(),
        Some(index) => trimmed[..index].to_string(),
        None => ".".to_string(),
    }
}

fn remote_name(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_string()
}

fn file_name_from_path(path: &str) -> String {
    remote_name(path)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

async fn read_remote_bytes_for_edit(
    server: &db::ServerConnectionConfig,
    remote_path: &str,
) -> Result<Vec<u8>, String> {
    const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;
    let handle = ssh::connect(server).await?;
    match sftp::read_file_prefix(&handle, remote_path, MAX_TEXT_BYTES + 1).await {
        Ok((bytes, _, truncated)) if !truncated && bytes.len() <= MAX_TEXT_BYTES => Ok(bytes),
        Ok(_) => Err("文本编辑器只支持不超过 2 MB 的文件。".to_string()),
        Err(error) if elevated_sftp::should_try_elevation(&error) => {
            let (encoded, _, truncated) =
                elevated_sftp::read_file_base64(server, remote_path, MAX_TEXT_BYTES + 1).await?;
            let bytes = decode_base64_lossy(&encoded);
            if truncated || bytes.len() > MAX_TEXT_BYTES {
                Err("文本编辑器只支持不超过 2 MB 的文件。".to_string())
            } else {
                Ok(bytes)
            }
        }
        Err(error) => Err(error),
    }
}

fn validate_remote_path(path: &str, destructive: bool) -> Result<(), String> {
    if path.is_empty() || !path.starts_with('/') {
        return Err("Remote path must be absolute".to_string());
    }
    if path.chars().any(char::is_control) {
        return Err("Remote path contains control characters".to_string());
    }
    if path.split('/').any(|part| matches!(part, "." | "..")) {
        return Err("Remote path traversal is not allowed".to_string());
    }
    if destructive && path.trim_end_matches('/').is_empty() {
        return Err("Refusing to modify the remote root directory".to_string());
    }
    Ok(())
}

fn validate_permission_mode(mode: &str) -> Result<(), String> {
    let valid =
        matches!(mode.len(), 3 | 4) && mode.chars().all(|character| matches!(character, '0'..='7'));
    if valid {
        Ok(())
    } else {
        Err("权限必须是 3 或 4 位八进制数字，例如 644、755 或 0755。".to_string())
    }
}

fn validate_tunnel_rule(input: &db::TunnelRuleInput) -> Result<(), String> {
    validate_text_field("隧道名称", &input.name, 128, false)?;
    validate_text_field("远程主机", &input.remote_host, 253, false)?;
    if input.remote_host.chars().any(char::is_whitespace) {
        return Err("远程主机不能包含空白字符。".to_string());
    }
    if input.local_host != "127.0.0.1"
        && input.local_host != "::1"
        && input.local_host != "localhost"
    {
        return Err("为避免意外暴露服务，本地隧道只允许监听回环地址。".to_string());
    }
    if !(1..=65_535).contains(&input.local_port) || !(1..=65_535).contains(&input.remote_port) {
        return Err("隧道端口必须在 1 到 65535 之间。".to_string());
    }
    Ok(())
}

fn mime_for_path(path: &str) -> String {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "pdf" => "application/pdf",
        "txt" | "log" | "md" | "json" | "toml" | "yaml" | "yml" | "xml" | "csv" | "ini"
        | "conf" | "sh" | "rs" | "ts" | "tsx" | "js" | "jsx" | "css" | "html" => "text/plain",
        "doc" | "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" | "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" | "pptx" => {
            "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        }
        _ => "application/octet-stream",
    }
    .to_string()
}

fn preview_kind_for_mime(mime: &str, path: &str) -> String {
    if mime.starts_with("image/") {
        "image"
    } else if mime.starts_with("video/") {
        "video"
    } else if mime.starts_with("audio/") {
        "audio"
    } else if mime == "application/pdf" {
        "pdf"
    } else if mime.starts_with("text/") || path.ends_with(".log") {
        "text"
    } else if mime.contains("wordprocessing")
        || mime.contains("spreadsheet")
        || mime.contains("presentation")
    {
        "document"
    } else {
        "unknown"
    }
    .to_string()
}

fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0b0000_0011) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b1 & 0b0000_1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn decode_base64_lossy(input: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0u8;
    for byte in input.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => continue,
        } as u32;
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    out
}

fn validate_create_secrets(input: &db::ServerInput) -> Result<(), String> {
    match input.auth_type.as_str() {
        "password" => {
            if password_is_supplied(input) {
                Ok(())
            } else {
                Err("密码登录需要输入密码，空密码服务器请勾选“此服务器使用空密码”。".to_string())
            }
        }
        "private_key" | "private_key_with_passphrase" => {
            if has_secret(input.private_key_content.as_deref())
                || has_secret(input.private_key_path.as_deref())
            {
                Ok(())
            } else {
                Err("私钥登录需要输入私钥路径或私钥内容。".to_string())
            }
        }
        value => Err(format!("Unsupported authentication type: {value}")),
    }
}

fn validate_update_secrets(
    database_path: &std::path::Path,
    server_id: &str,
    input: &db::ServerInput,
) -> Result<(), String> {
    let existing =
        db::get_server_secret_state(database_path, server_id).map_err(|error| error.to_string())?;

    match input.auth_type.as_str() {
        "password" => {
            if password_is_supplied(input) {
                return Ok(());
            }

            if existing.auth_type == "password" {
                ensure_existing_secret(
                    existing.credential_ref.as_deref(),
                    "该服务器保存的密码凭据不存在，请重新输入密码后保存。",
                )
            } else {
                Err("切换为密码登录时需要重新输入密码。".to_string())
            }
        }
        "private_key" => {
            if has_secret(input.private_key_content.as_deref())
                || has_secret(input.private_key_path.as_deref())
                || existing_has_private_key(&existing)
            {
                Ok(())
            } else {
                Err("私钥登录需要输入私钥路径或私钥内容。".to_string())
            }
        }
        "private_key_with_passphrase" => {
            if !(has_secret(input.private_key_content.as_deref())
                || has_secret(input.private_key_path.as_deref())
                || existing_has_private_key(&existing))
            {
                return Err("私钥登录需要输入私钥路径或私钥内容。".to_string());
            }

            if has_secret(input.passphrase.as_deref()) {
                return Ok(());
            }

            if existing.auth_type == "private_key_with_passphrase" {
                ensure_existing_secret(
                    existing.credential_ref.as_deref(),
                    "该服务器保存的 passphrase 凭据不存在，请重新输入 passphrase 后保存。",
                )
            } else {
                Err("切换为私钥 + passphrase 时需要重新输入 passphrase。".to_string())
            }
        }
        value => Err(format!("Unsupported authentication type: {value}")),
    }
}

fn ensure_existing_secret(reference: Option<&str>, message: &str) -> Result<(), String> {
    let reference = reference.ok_or_else(|| message.to_string())?;
    credentials::read_secret(reference)
        .map(|_| ())
        .map_err(|_| message.to_string())
}

fn existing_has_private_key(existing: &db::ServerSecretState) -> bool {
    matches!(
        existing.auth_type.as_str(),
        "private_key" | "private_key_with_passphrase"
    ) && (existing.private_key_ref.is_some() || has_secret(existing.private_key_path.as_deref()))
}

fn has_secret(value: Option<&str>) -> bool {
    value.map(|value| !value.trim().is_empty()).unwrap_or(false)
}

fn password_is_supplied(input: &db::ServerInput) -> bool {
    input.use_empty_password || has_secret(input.password.as_deref())
}

fn save_input_secrets(
    server_id: &str,
    input: &db::ServerInput,
) -> Result<(Option<String>, Option<String>), String> {
    let credential_secret = match input.auth_type.as_str() {
        "password" if input.use_empty_password => Some(""),
        "password" => input
            .password
            .as_deref()
            .filter(|secret| !secret.trim().is_empty()),
        "private_key_with_passphrase" => input
            .passphrase
            .as_deref()
            .filter(|secret| !secret.trim().is_empty()),
        _ => None,
    };
    let credential_ref = if let Some(secret) = credential_secret {
        let reference = credentials::create_secret_ref(server_id, "credential");
        credentials::save_secret(&reference, secret).map_err(|error| {
            format!("无法保存密码到系统安全凭据，请确认系统凭据存储可用。详细信息: {error}")
        })?;
        Some(reference)
    } else {
        None
    };

    let private_key_ref = if let Some(secret) = input.private_key_content.as_ref() {
        if secret.is_empty() {
            None
        } else {
            let reference = credentials::create_secret_ref(server_id, "private-key");
            if let Err(error) = credentials::save_secret(&reference, secret) {
                delete_secret_refs([credential_ref.as_deref()]);
                return Err(format!(
                    "无法保存私钥到系统安全凭据，请确认 Windows 凭据管理器可用。详细信息: {error}"
                ));
            }
            Some(reference)
        }
    } else {
        None
    };

    Ok((credential_ref, private_key_ref))
}

fn copy_secret(source_ref: &str, server_id: &str, kind: &str) -> Result<String, String> {
    let secret = credentials::read_secret(source_ref)
        .map_err(|error| format!("无法复制系统安全凭据：{error}"))?;
    let target_ref = credentials::create_secret_ref(server_id, kind);
    credentials::save_secret(&target_ref, &secret)
        .map_err(|error| format!("无法保存复制的系统安全凭据：{error}"))?;
    Ok(target_ref)
}

fn delete_secret_refs<'a>(references: impl IntoIterator<Item = Option<&'a str>>) {
    for reference in references.into_iter().flatten() {
        let _ = credentials::delete_secret(reference);
    }
}

#[cfg(test)]
mod tests {
    use super::{password_is_supplied, validate_permission_mode, validate_remote_path};
    use crate::db::ServerInput;

    fn password_input(password: Option<&str>, use_empty_password: bool) -> ServerInput {
        ServerInput {
            id: None,
            name: "test".to_string(),
            host: "127.0.0.1".to_string(),
            port: 22,
            username: "root".to_string(),
            auth_type: "password".to_string(),
            password: password.map(str::to_string),
            use_empty_password,
            private_key_path: None,
            private_key_content: None,
            passphrase: None,
            group_id: None,
            tags: Vec::new(),
            note: String::new(),
        }
    }

    #[test]
    fn accepts_explicit_empty_password_only() {
        assert!(password_is_supplied(&password_input(None, true)));
        assert!(password_is_supplied(&password_input(Some("secret"), false)));
        assert!(!password_is_supplied(&password_input(None, false)));
        assert!(!password_is_supplied(&password_input(Some(""), false)));
    }

    #[test]
    fn blocks_dangerous_remote_paths() {
        assert!(validate_remote_path("/", true).is_err());
        assert!(validate_remote_path("/data/../etc", false).is_err());
        assert!(validate_remote_path("relative/path", false).is_err());
        assert!(validate_remote_path("/data/file.txt", false).is_ok());
    }

    #[test]
    fn validates_octal_permissions() {
        assert!(validate_permission_mode("644").is_ok());
        assert!(validate_permission_mode("0755").is_ok());
        assert!(validate_permission_mode("888").is_err());
        assert!(validate_permission_mode("7555x").is_err());
    }
}
