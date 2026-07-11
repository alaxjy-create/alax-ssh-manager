use crate::{commands::TerminalSession, credentials, db::ServerConnectionConfig};
use russh::keys::ssh_key::HashAlg;
use russh::keys::{decode_secret_key, load_secret_key, PrivateKeyWithHashAlg};
use russh::{client, ChannelMsg, Disconnect};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc::{self, Sender};
use tokio::time::timeout;
use uuid::Uuid;

pub type SshHandle = client::Handle<SshClientHandler>;

#[derive(Debug)]
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_status: Option<u32>,
}

impl CommandResult {
    pub fn combined_output(&self) -> String {
        match (self.stdout.trim().is_empty(), self.stderr.trim().is_empty()) {
            (true, true) => String::new(),
            (false, true) => self.stdout.clone(),
            (true, false) => self.stderr.clone(),
            (false, false) => format!("{}{}", self.stdout, self.stderr),
        }
    }

    pub fn is_success(&self) -> bool {
        self.exit_status == Some(0)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyInfo {
    pub algorithm: String,
    pub fingerprint: String,
}

#[derive(Debug)]
pub enum TerminalCommand {
    Write(String),
    Resize { cols: u32, rows: u32 },
    Close,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOutputEvent {
    pub session_id: String,
    pub data: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalStatusEvent {
    pub session_id: String,
    pub status: String,
    pub message: Option<String>,
}

pub struct SshClientHandler {
    expected_fingerprint: Option<String>,
    allow_unknown: bool,
    observed: Arc<Mutex<Option<HostKeyInfo>>>,
}

impl client::Handler for SshClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        let info = HostKeyInfo {
            algorithm: server_public_key.algorithm().to_string(),
            fingerprint: server_public_key.fingerprint(HashAlg::Sha256).to_string(),
        };
        if let Ok(mut observed) = self.observed.lock() {
            *observed = Some(info.clone());
        }
        Ok(self.allow_unknown
            || self
                .expected_fingerprint
                .as_deref()
                .is_some_and(|expected| expected == info.fingerprint))
    }
}

pub async fn connect(config: &ServerConnectionConfig) -> Result<SshHandle, String> {
    let (mut handle, _) = connect_transport(config, false).await?;
    timeout(Duration::from_secs(20), authenticate(&mut handle, config))
        .await
        .map_err(|_| "SSH authentication timed out".to_string())??;

    Ok(handle)
}

pub async fn probe_host_key(config: &ServerConnectionConfig) -> Result<HostKeyInfo, String> {
    let (handle, info) = connect_transport(config, true).await?;
    disconnect(&handle).await;
    Ok(info)
}

async fn connect_transport(
    config: &ServerConnectionConfig,
    allow_unknown: bool,
) -> Result<(SshHandle, HostKeyInfo), String> {
    let port =
        u16::try_from(config.port).map_err(|_| format!("Invalid SSH port: {}", config.port))?;
    let client_config = client::Config {
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 3,
        inactivity_timeout: Some(Duration::from_secs(10 * 60)),
        ..Default::default()
    };
    let arc_config = Arc::new(client_config);
    let observed = Arc::new(Mutex::new(None));
    let handler = SshClientHandler {
        expected_fingerprint: config.trusted_host_key.clone(),
        allow_unknown,
        observed: observed.clone(),
    };
    let connected = timeout(
        Duration::from_secs(12),
        client::connect(arc_config, (config.host.as_str(), port), handler),
    )
    .await
    .map_err(|_| "SSH connection timed out".to_string())?;

    let info = observed
        .lock()
        .map_err(|_| "Unable to inspect SSH host key".to_string())?
        .clone();

    let handle = match connected {
        Ok(handle) => handle,
        Err(error) => {
            if let Some(info) = info {
                if let Some(expected) = config.trusted_host_key.as_deref() {
                    if expected != info.fingerprint {
                        return Err(format!(
                            "SSH 主机密钥已变化，连接已阻止。期望 {expected}，实际 {}。请确认服务器身份后重新信任。",
                            info.fingerprint
                        ));
                    }
                } else if !allow_unknown {
                    return Err(format!(
                        "SSH 主机密钥尚未信任：{} {}",
                        info.algorithm, info.fingerprint
                    ));
                }
            }
            return Err(format!("Unable to connect to SSH server: {error}"));
        }
    };

    let info = info.ok_or_else(|| "SSH server did not provide a host key".to_string())?;
    Ok((handle, info))
}

pub async fn run_command(config: &ServerConnectionConfig, command: &str) -> Result<String, String> {
    run_command_with_timeout(config, command, Duration::from_secs(60)).await
}

pub async fn run_command_with_timeout(
    config: &ServerConnectionConfig,
    command: &str,
    command_timeout: Duration,
) -> Result<String, String> {
    let result = run_command_with_input_timeout(config, command, None, command_timeout).await?;
    if result.is_success() {
        Ok(result.combined_output())
    } else {
        Err(format!(
            "Remote command failed with status {}: {}",
            result.exit_status.unwrap_or(255),
            result.combined_output().trim()
        ))
    }
}

pub async fn run_command_with_input(
    config: &ServerConnectionConfig,
    command: &str,
    stdin: Option<&str>,
) -> Result<CommandResult, String> {
    run_command_with_input_timeout(config, command, stdin, Duration::from_secs(60)).await
}

pub async fn run_command_with_input_timeout(
    config: &ServerConnectionConfig,
    command: &str,
    stdin: Option<&str>,
    command_timeout: Duration,
) -> Result<CommandResult, String> {
    let handle = connect(config).await?;
    let result =
        run_command_on_handle_with_input_timeout(&handle, command, stdin, command_timeout).await;
    disconnect(&handle).await;
    result
}

pub async fn run_command_on_handle_with_input_timeout(
    handle: &SshHandle,
    command: &str,
    stdin: Option<&str>,
    command_timeout: Duration,
) -> Result<CommandResult, String> {
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|error| format!("Unable to open SSH channel: {error}"))?;
    channel
        .exec(true, command)
        .await
        .map_err(|error| format!("Unable to execute command: {error}"))?;
    let (mut reader, writer) = channel.split();

    if let Some(input) = stdin {
        writer
            .data(input.as_bytes())
            .await
            .map_err(|error| format!("Unable to write command input: {error}"))?;
        let _ = writer.eof().await;
    }

    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit_status = None;

    timeout(command_timeout, async {
        loop {
            match reader.wait().await {
                Some(ChannelMsg::Data { data }) => {
                    stdout.push_str(&String::from_utf8_lossy(&data));
                    if stdout.len() > 16 * 1024 * 1024 {
                        return Err("Remote command output exceeded 16 MB".to_string());
                    }
                }
                Some(ChannelMsg::ExtendedData { data, .. }) => {
                    stderr.push_str(&String::from_utf8_lossy(&data));
                    if stderr.len() > 16 * 1024 * 1024 {
                        return Err("Remote command error output exceeded 16 MB".to_string());
                    }
                }
                Some(ChannelMsg::ExitStatus {
                    exit_status: status,
                }) => {
                    exit_status = Some(status);
                }
                Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break Ok(()),
                _ => {}
            }
        }
    })
    .await
    .map_err(|_| {
        format!(
            "Remote command timed out after {} seconds",
            command_timeout.as_secs()
        )
    })??;

    Ok(CommandResult {
        stdout,
        stderr,
        exit_status,
    })
}

async fn authenticate(
    handle: &mut SshHandle,
    config: &ServerConnectionConfig,
) -> Result<(), String> {
    match config.auth_type.as_str() {
        "password" => {
            let password = read_required_secret(
                config.credential_ref.as_deref(),
                "Missing password credential",
            )
            .await?;
            let result = handle
                .authenticate_password(&config.username, &password)
                .await
                .map_err(|error| format!("Password authentication failed: {error}"))?;
            if !result.success() {
                authenticate_keyboard_interactive(handle, &config.username, &password).await?;
            }
            Ok(())
        }
        "private_key" | "private_key_with_passphrase" => {
            let passphrase = match config.credential_ref.as_deref() {
                Some(reference) => {
                    Some(credentials::read_secret_async(reference.to_string()).await?)
                }
                None => None,
            };

            let key_pair = if let Some(reference) = config.private_key_ref.as_deref() {
                let private_key = credentials::read_secret_async(reference.to_string()).await?;
                decode_secret_key(&private_key, passphrase.as_deref())
                    .map_err(|error| format!("Unable to decode private key: {error}"))?
            } else if let Some(path) = config.private_key_path.as_deref() {
                load_secret_key(path, passphrase.as_deref())
                    .map_err(|error| format!("Unable to load private key file: {error}"))?
            } else {
                return Err("Missing private key path or private key content".to_string());
            };

            let hash = handle
                .best_supported_rsa_hash()
                .await
                .map_err(|error| format!("Unable to negotiate RSA hash: {error}"))?
                .flatten();

            let result = handle
                .authenticate_publickey(
                    &config.username,
                    PrivateKeyWithHashAlg::new(Arc::new(key_pair), hash),
                )
                .await
                .map_err(|error| format!("Private key authentication failed: {error}"))?;

            if !result.success() {
                return Err("Private key authentication rejected by server".to_string());
            }
            Ok(())
        }
        value => Err(format!("Unsupported authentication type: {value}")),
    }
}

async fn authenticate_keyboard_interactive(
    handle: &mut SshHandle,
    username: &str,
    password: &str,
) -> Result<(), String> {
    let mut response = handle
        .authenticate_keyboard_interactive_start(username, None)
        .await
        .map_err(|error| format!("Keyboard-interactive authentication failed: {error}"))?;

    for _ in 0..8 {
        match response {
            client::KeyboardInteractiveAuthResponse::Success => return Ok(()),
            client::KeyboardInteractiveAuthResponse::Failure { .. } => {
                return Err("Password authentication rejected by server".to_string());
            }
            client::KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                let answers = prompts
                    .iter()
                    .map(|prompt| {
                        if prompt.echo {
                            String::new()
                        } else {
                            password.to_string()
                        }
                    })
                    .collect();
                response = handle
                    .authenticate_keyboard_interactive_respond(answers)
                    .await
                    .map_err(|error| {
                        format!("Keyboard-interactive authentication failed: {error}")
                    })?;
            }
        }
    }

    Err("Keyboard-interactive authentication exceeded prompt limit".to_string())
}

async fn read_required_secret(reference: Option<&str>, message: &str) -> Result<String, String> {
    let reference = reference.ok_or_else(|| message.to_string())?;
    credentials::read_secret_async(reference.to_string()).await
}

pub fn spawn_pty(
    app: AppHandle,
    config: ServerConnectionConfig,
    cols: u32,
    rows: u32,
) -> (TerminalSession, Sender<TerminalCommand>) {
    let session_id = Uuid::new_v4().to_string();
    let server_id = config.id.clone();
    let (tx, mut rx) = mpsc::channel::<TerminalCommand>(64);
    let thread_session_id = session_id.clone();

    tokio::spawn(async move {
        tokio::task::yield_now().await;
        emit_status(
            &app,
            &thread_session_id,
            "connecting",
            Some("Connecting to SSH server"),
        );

        let handle = match connect(&config).await {
            Ok(handle) => handle,
            Err(error) => {
                emit_status(&app, &thread_session_id, "error", Some(&error));
                return;
            }
        };

        let channel = match handle.channel_open_session().await {
            Ok(channel) => channel,
            Err(error) => {
                emit_status(
                    &app,
                    &thread_session_id,
                    "error",
                    Some(&format!("Unable to create SSH channel: {error}")),
                );
                disconnect(&handle).await;
                return;
            }
        };

        if let Err(error) = channel
            .request_pty(
                false,
                "xterm-256color",
                cols.max(20),
                rows.max(5),
                0,
                0,
                &[],
            )
            .await
        {
            emit_status(
                &app,
                &thread_session_id,
                "error",
                Some(&format!("Unable to request PTY: {error}")),
            );
            disconnect(&handle).await;
            return;
        }

        if let Err(error) = channel.request_shell(true).await {
            emit_status(
                &app,
                &thread_session_id,
                "error",
                Some(&format!("Unable to start remote shell: {error}")),
            );
            disconnect(&handle).await;
            return;
        }

        let (mut reader, writer) = channel.split();
        emit_status(
            &app,
            &thread_session_id,
            "connected",
            Some("SSH PTY connected"),
        );

        loop {
            tokio::select! {
                command = rx.recv() => {
                    match command {
                        Some(TerminalCommand::Write(data)) => {
                            if let Err(error) = writer.data(data.as_bytes()).await {
                                emit_status(&app, &thread_session_id, "error", Some(&format!("Unable to write terminal input: {error}")));
                                disconnect(&handle).await;
                                return;
                            }
                        }
                        Some(TerminalCommand::Resize { cols, rows }) => {
                            let _ = writer.window_change(cols.max(20), rows.max(5), 0, 0).await;
                        }
                        Some(TerminalCommand::Close) | None => {
                            let _ = writer.eof().await;
                            let _ = writer.close().await;
                            let _ = handle.disconnect(Disconnect::ByApplication, "", "English").await;
                            emit_status(&app, &thread_session_id, "closed", Some("Terminal closed"));
                            return;
                        }
                    }
                }
                message = reader.wait() => {
                    match message {
                        Some(ChannelMsg::Data { data }) => {
                            let text = String::from_utf8_lossy(&data).to_string();
                            let _ = app.emit(
                                "terminal-output",
                                TerminalOutputEvent {
                                    session_id: thread_session_id.clone(),
                                    data: text,
                                },
                            );
                        }
                        Some(ChannelMsg::ExtendedData { data, .. }) => {
                            let text = String::from_utf8_lossy(&data).to_string();
                            let _ = app.emit(
                                "terminal-output",
                                TerminalOutputEvent {
                                    session_id: thread_session_id.clone(),
                                    data: text,
                                },
                            );
                        }
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                            let _ = handle.disconnect(Disconnect::ByApplication, "", "English").await;
                            emit_status(&app, &thread_session_id, "closed", Some("Remote shell ended"));
                            return;
                        }
                        Some(ChannelMsg::ExitStatus { exit_status }) => {
                            let message = format!("Remote shell exited with status {exit_status}");
                            let _ = handle.disconnect(Disconnect::ByApplication, "", "English").await;
                            emit_status(&app, &thread_session_id, "closed", Some(&message));
                            return;
                        }
                        Some(ChannelMsg::ExitSignal { signal_name, error_message, .. }) => {
                            let message = if error_message.is_empty() {
                                format!("Remote shell exited by signal {signal_name:?}")
                            } else {
                                format!("Remote shell exited by signal {signal_name:?}: {error_message}")
                            };
                            let _ = handle.disconnect(Disconnect::ByApplication, "", "English").await;
                            emit_status(&app, &thread_session_id, "closed", Some(&message));
                            return;
                        }
                        Some(ChannelMsg::Failure) => {
                            let _ = handle.disconnect(Disconnect::ByApplication, "", "English").await;
                            emit_status(&app, &thread_session_id, "error", Some("Remote shell request failed"));
                            return;
                        }
                        Some(ChannelMsg::OpenFailure(reason)) => {
                            let _ = handle.disconnect(Disconnect::ByApplication, "", "English").await;
                            emit_status(&app, &thread_session_id, "error", Some(&format!("Remote channel open failed: {reason:?}")));
                            return;
                        }
                        Some(ChannelMsg::Success) | Some(ChannelMsg::WindowAdjusted { .. }) | Some(ChannelMsg::XonXoff { .. }) => {}
                        _ => {
                            tokio::task::yield_now().await;
                        }
                    }
                }
            }
        }
    });

    (
        TerminalSession {
            id: session_id,
            server_id,
            status: "connecting".to_string(),
        },
        tx,
    )
}

pub async fn close_session(sender: &Sender<TerminalCommand>) {
    let _ = sender.send(TerminalCommand::Close).await;
}

#[allow(dead_code)]
fn _read_required_secret_sync(_reference: Option<&str>, _message: &str) -> Result<String, String> {
    unreachable!("use read_required_secret async version instead")
}

pub async fn disconnect(handle: &SshHandle) {
    let _ = handle
        .disconnect(Disconnect::ByApplication, "", "English")
        .await;
}

fn emit_status(app: &AppHandle, session_id: &str, status: &str, message: Option<&str>) {
    let _ = app.emit(
        "terminal-status",
        TerminalStatusEvent {
            session_id: session_id.to_string(),
            status: status.to_string(),
            message: message.map(str::to_string),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::CommandResult;

    #[test]
    fn missing_exit_status_is_not_success() {
        assert!(!CommandResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_status: None,
        }
        .is_success());
    }
}
