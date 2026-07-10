use crate::{db::ServerConnectionConfig, elevated_sftp, sftp, ssh};
use flate2::{write::GzEncoder, Compression};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tar::{Archive, Builder};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferInput {
    pub server_id: String,
    pub transfer_type: String,
    pub local_path: String,
    pub remote_path: String,
    #[serde(default = "default_entry_kind")]
    pub entry_kind: String,
}

fn default_entry_kind() -> String {
    "file".to_string()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferTask {
    pub id: String,
    pub server_id: String,
    pub transfer_type: String,
    pub local_path: String,
    pub remote_path: String,
    pub status: String,
    pub progress: f64,
    pub speed: i64,
    pub error_message: Option<String>,
}

#[derive(Clone)]
pub struct TransferHandle {
    pub input: TransferInput,
    pub cancel: Arc<AtomicBool>,
    pub active: Arc<AtomicBool>,
}

struct ActiveTransferGuard(Arc<AtomicBool>);

impl Drop for ActiveTransferGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgressEvent {
    pub id: String,
    pub status: String,
    pub progress: f64,
    pub speed: i64,
    pub error_message: Option<String>,
}

pub fn new_task(input: TransferInput) -> TransferTask {
    TransferTask {
        id: Uuid::new_v4().to_string(),
        server_id: input.server_id,
        transfer_type: input.transfer_type,
        local_path: input.local_path,
        remote_path: input.remote_path,
        status: "running".to_string(),
        progress: 0.0,
        speed: 0,
        error_message: None,
    }
}

pub fn spawn_transfer(
    app: AppHandle,
    task_id: String,
    config: ServerConnectionConfig,
    input: TransferInput,
    cancel: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
) {
    tokio::spawn(async move {
        let _active_guard = ActiveTransferGuard(active);
        emit_progress(&app, &task_id, "running", 0.0, 0, None);

        if input.entry_kind == "directory" {
            let started_at = Instant::now();
            let mut last_emit = Instant::now();
            let mut latest_progress = 0.0;
            let mut latest_speed = 0_i64;
            let result = match input.transfer_type.as_str() {
                "upload" => {
                    upload_directory_archive(&config, &input, &cancel, |done, total, _chunk| {
                        update_progress(
                            done,
                            total,
                            started_at,
                            &mut latest_progress,
                            &mut latest_speed,
                        );
                        if last_emit.elapsed() >= Duration::from_millis(150) || done == total {
                            emit_progress(
                                &app,
                                &task_id,
                                "running",
                                latest_progress,
                                latest_speed,
                                None,
                            );
                            last_emit = Instant::now();
                        }
                    })
                    .await
                }
                "download" => {
                    download_directory_archive(&config, &input, &cancel, |done, total, _chunk| {
                        update_progress(
                            done,
                            total,
                            started_at,
                            &mut latest_progress,
                            &mut latest_speed,
                        );
                        if last_emit.elapsed() >= Duration::from_millis(150) || done == total {
                            emit_progress(
                                &app,
                                &task_id,
                                "running",
                                latest_progress,
                                latest_speed,
                                None,
                            );
                            last_emit = Instant::now();
                        }
                    })
                    .await
                }
                other => Err(format!("Unsupported transfer type: {other}")),
            };

            match result {
                Ok(()) => emit_progress(&app, &task_id, "done", 100.0, latest_speed, None),
                Err(error) => {
                    let status = if cancel.load(Ordering::Relaxed) {
                        "cancelled"
                    } else {
                        "failed"
                    };
                    emit_progress(&app, &task_id, status, latest_progress, 0, Some(error));
                }
            }
            return;
        }

        let handle = match ssh::connect(&config).await {
            Ok(handle) => handle,
            Err(error) => {
                emit_progress(&app, &task_id, "failed", 0.0, 0, Some(error));
                return;
            }
        };

        let started_at = Instant::now();
        let mut last_emit = Instant::now();
        let mut latest_progress = 0.0;
        let mut latest_speed = 0_i64;

        let result = match input.transfer_type.as_str() {
            "upload" => {
                sftp::upload_file_with_progress(
                    &handle,
                    &input.local_path,
                    &input.remote_path,
                    &cancel,
                    |done, total, _chunk| {
                        update_progress(
                            done,
                            total,
                            started_at,
                            &mut latest_progress,
                            &mut latest_speed,
                        );
                        if last_emit.elapsed() >= Duration::from_millis(150) || done == total {
                            emit_progress(
                                &app,
                                &task_id,
                                "running",
                                latest_progress,
                                latest_speed,
                                None,
                            );
                            last_emit = Instant::now();
                        }
                    },
                )
                .await
            }
            "download" => {
                sftp::download_file_with_progress(
                    &handle,
                    &input.remote_path,
                    &input.local_path,
                    &cancel,
                    |done, total, _chunk| {
                        update_progress(
                            done,
                            total,
                            started_at,
                            &mut latest_progress,
                            &mut latest_speed,
                        );
                        if last_emit.elapsed() >= Duration::from_millis(150) || done == total {
                            emit_progress(
                                &app,
                                &task_id,
                                "running",
                                latest_progress,
                                latest_speed,
                                None,
                            );
                            last_emit = Instant::now();
                        }
                    },
                )
                .await
            }
            other => {
                ssh::disconnect(&handle).await;
                Err(format!("Unsupported transfer type: {other}"))
            }
        };

        let result = match result {
            Ok(()) => Ok(()),
            Err(error)
                if !cancel.load(Ordering::Relaxed)
                    && elevated_sftp::should_try_elevation(&error) =>
            {
                emit_progress(
                    &app,
                    &task_id,
                    "running",
                    latest_progress,
                    latest_speed,
                    None,
                );
                match input.transfer_type.as_str() {
                    "upload" => {
                        elevated_sftp::upload_file_with_progress(
                            &config,
                            &input.local_path,
                            &input.remote_path,
                            &cancel,
                            |done, total, _chunk| {
                                update_progress(
                                    done,
                                    total,
                                    started_at,
                                    &mut latest_progress,
                                    &mut latest_speed,
                                );
                                if last_emit.elapsed() >= Duration::from_millis(150)
                                    || done == total
                                {
                                    emit_progress(
                                        &app,
                                        &task_id,
                                        "running",
                                        latest_progress,
                                        latest_speed,
                                        None,
                                    );
                                    last_emit = Instant::now();
                                }
                            },
                        )
                        .await
                    }
                    "download" => {
                        elevated_sftp::download_file_with_progress(
                            &config,
                            &input.remote_path,
                            &input.local_path,
                            &cancel,
                            |done, total, _chunk| {
                                update_progress(
                                    done,
                                    total,
                                    started_at,
                                    &mut latest_progress,
                                    &mut latest_speed,
                                );
                                if last_emit.elapsed() >= Duration::from_millis(150)
                                    || done == total
                                {
                                    emit_progress(
                                        &app,
                                        &task_id,
                                        "running",
                                        latest_progress,
                                        latest_speed,
                                        None,
                                    );
                                    last_emit = Instant::now();
                                }
                            },
                        )
                        .await
                    }
                    _ => Err(error),
                }
            }
            Err(error) => Err(error),
        };

        let result = match result {
            Ok(()) if input.transfer_type == "upload" => {
                match elevated_sftp::remote_path_exists(&config, &input.remote_path).await {
                    Ok(true) => Ok(()),
                    Ok(false) if !cancel.load(Ordering::Relaxed) => {
                        elevated_sftp::upload_file_with_progress(
                            &config,
                            &input.local_path,
                            &input.remote_path,
                            &cancel,
                            |done, total, _chunk| {
                                update_progress(
                                    done,
                                    total,
                                    started_at,
                                    &mut latest_progress,
                                    &mut latest_speed,
                                );
                                if last_emit.elapsed() >= Duration::from_millis(150)
                                    || done == total
                                {
                                    emit_progress(
                                        &app,
                                        &task_id,
                                        "running",
                                        latest_progress,
                                        latest_speed,
                                        None,
                                    );
                                    last_emit = Instant::now();
                                }
                            },
                        )
                        .await
                    }
                    Ok(false) => Err("Remote file was not uploaded".to_string()),
                    Err(error) => Err(format!("Upload verification failed: {error}")),
                }
            }
            other => other,
        };

        match result {
            Ok(()) => emit_progress(&app, &task_id, "done", 100.0, latest_speed, None),
            Err(error) => {
                let status = if cancel.load(Ordering::Relaxed) {
                    "cancelled"
                } else {
                    "failed"
                };
                emit_progress(&app, &task_id, status, latest_progress, 0, Some(error));
            }
        }
    });
}

async fn upload_directory_archive<F>(
    config: &ServerConnectionConfig,
    input: &TransferInput,
    cancel: &AtomicBool,
    progress: F,
) -> Result<(), String>
where
    F: FnMut(u64, u64, u64),
{
    let source = PathBuf::from(&input.local_path);
    if !source.is_dir() {
        return Err("Selected upload path is not a directory".to_string());
    }
    let local_archive = std::env::temp_dir().join(format!("alax-upload-{}.tar.gz", Uuid::new_v4()));
    let source_for_archive = source.clone();
    let archive_for_task = local_archive.clone();
    tokio::task::spawn_blocking(move || {
        create_directory_archive(&source_for_archive, &archive_for_task)
    })
    .await
    .map_err(|error| format!("Directory compression task failed: {error}"))??;

    let remote_archive = format!("/tmp/alax-upload-{}.tar.gz", Uuid::new_v4());
    let handle = ssh::connect(config).await?;
    let upload_result = sftp::upload_file_with_progress(
        &handle,
        &local_archive.to_string_lossy(),
        &remote_archive,
        cancel,
        progress,
    )
    .await;
    let _ = tokio::fs::remove_file(&local_archive).await;
    upload_result?;
    if cancel.load(Ordering::Relaxed) {
        return Err("Transfer cancelled".to_string());
    }

    let parent = remote_parent(&input.remote_path);
    let script = "mkdir -p -- \"$1\" && tar -xzf \"$2\" -C \"$1\"; status=$?; if [ \"$status\" -eq 0 ]; then rm -f -- \"$2\"; fi; exit \"$status\"";
    let command = format!(
        "sh -c {} sh {} {}",
        shell_quote(script),
        shell_quote(&parent),
        shell_quote(&remote_archive)
    );
    match ssh::run_command_with_timeout(config, &command, Duration::from_secs(30 * 60)).await {
        Ok(_) => {}
        Err(error) if elevated_sftp::should_try_elevation(&error) => {
            elevated_sftp::extract_uploaded_directory(config, &remote_archive, &parent).await?;
        }
        Err(error) => return Err(error),
    }
    match elevated_sftp::remote_path_exists(config, &input.remote_path).await {
        Ok(true) => Ok(()),
        Ok(false) => Err("Remote directory was not created after upload".to_string()),
        Err(error) => Err(format!("Unable to verify uploaded directory: {error}")),
    }
}

async fn download_directory_archive<F>(
    config: &ServerConnectionConfig,
    input: &TransferInput,
    cancel: &AtomicBool,
    progress: F,
) -> Result<(), String>
where
    F: FnMut(u64, u64, u64),
{
    let remote_archive = format!("/tmp/alax-download-{}.tar.gz", Uuid::new_v4());
    let parent = remote_parent(&input.remote_path);
    let name = remote_name(&input.remote_path);
    let script = "tar -czf \"$1\" -C \"$2\" -- \"$3\" && chmod 600 -- \"$1\"";
    let command = format!(
        "sh -c {} sh {} {} {}",
        shell_quote(script),
        shell_quote(&remote_archive),
        shell_quote(&parent),
        shell_quote(&name)
    );
    match ssh::run_command_with_timeout(config, &command, Duration::from_secs(30 * 60)).await {
        Ok(_) => {}
        Err(error) if elevated_sftp::should_try_elevation(&error) => {
            elevated_sftp::prepare_directory_download(config, &input.remote_path, &remote_archive)
                .await?;
        }
        Err(error) => return Err(error),
    }

    let local_archive =
        std::env::temp_dir().join(format!("alax-download-{}.tar.gz", Uuid::new_v4()));
    let handle = ssh::connect(config).await?;
    let download_result = sftp::download_file_with_progress(
        &handle,
        &remote_archive,
        &local_archive.to_string_lossy(),
        cancel,
        progress,
    )
    .await;
    let cleanup_command = format!("rm -f -- {}", shell_quote(&remote_archive));
    let _ = ssh::run_command(config, &cleanup_command).await;
    download_result?;
    if cancel.load(Ordering::Relaxed) {
        let _ = tokio::fs::remove_file(&local_archive).await;
        return Err("Transfer cancelled".to_string());
    }

    let destination = PathBuf::from(&input.local_path);
    tokio::fs::create_dir_all(&destination)
        .await
        .map_err(|error| format!("Unable to create local destination: {error}"))?;
    let archive_for_task = local_archive.clone();
    let destination_for_task = destination.clone();
    let extract_result = tokio::task::spawn_blocking(move || {
        extract_directory_archive(&archive_for_task, &destination_for_task)
    })
    .await
    .map_err(|error| format!("Directory extraction task failed: {error}"))?;
    let _ = tokio::fs::remove_file(&local_archive).await;
    extract_result
}

fn create_directory_archive(source: &Path, archive_path: &Path) -> Result<(), String> {
    let name = source
        .file_name()
        .ok_or_else(|| "Unable to determine directory name".to_string())?;
    let file = File::create(archive_path)
        .map_err(|error| format!("Unable to create local archive: {error}"))?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut archive = Builder::new(encoder);
    archive.follow_symlinks(false);
    archive
        .append_dir_all(name, source)
        .map_err(|error| format!("Unable to archive directory: {error}"))?;
    let encoder = archive
        .into_inner()
        .map_err(|error| format!("Unable to finish archive: {error}"))?;
    encoder
        .finish()
        .map_err(|error| format!("Unable to finish compression: {error}"))?;
    Ok(())
}

fn extract_directory_archive(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let file = File::open(archive_path)
        .map_err(|error| format!("Unable to open downloaded archive: {error}"))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    archive
        .unpack(destination)
        .map_err(|error| format!("Unable to extract downloaded directory: {error}"))
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
        .unwrap_or("directory")
        .to_string()
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn update_progress(
    done: u64,
    total: u64,
    started_at: Instant,
    progress: &mut f64,
    speed: &mut i64,
) {
    *progress = if total > 0 {
        ((done as f64 / total as f64) * 100.0).clamp(0.0, 100.0)
    } else if done > 0 {
        100.0
    } else {
        0.0
    };

    let elapsed = started_at.elapsed().as_secs_f64().max(0.001);
    *speed = (done as f64 / elapsed) as i64;
}

fn emit_progress(
    app: &AppHandle,
    id: &str,
    status: &str,
    progress: f64,
    speed: i64,
    error_message: Option<String>,
) {
    let _ = app.emit(
        "transfer-progress",
        TransferProgressEvent {
            id: id.to_string(),
            status: status.to_string(),
            progress,
            speed,
            error_message,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::{create_directory_archive, extract_directory_archive};
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn directory_archive_round_trip_preserves_nested_files() {
        let root = std::env::temp_dir().join(format!("alax-archive-test-{}", Uuid::new_v4()));
        let source = root.join("source");
        let nested = source.join("nested");
        let destination = root.join("destination");
        let archive = root.join("source.tar.gz");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("hello.txt"), b"hello").unwrap();

        create_directory_archive(&source, &archive).unwrap();
        fs::create_dir_all(&destination).unwrap();
        extract_directory_archive(&archive, &destination).unwrap();

        assert_eq!(
            fs::read(destination.join("source/nested/hello.txt")).unwrap(),
            b"hello"
        );
        let _ = fs::remove_dir_all(root);
    }
}
