use crate::ssh::{disconnect, SshHandle};
use russh_sftp::client::{error::Error as SftpError, SftpSession};
use russh_sftp::protocol::{OpenFlags, StatusCode};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};
use uuid::Uuid;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFileEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub kind: String,
    pub size: i64,
    pub modified_at: String,
    pub permissions: String,
    pub owner: String,
}

async fn open_sftp(handle: &SshHandle) -> Result<SftpSession, String> {
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|error| format!("Unable to open SFTP channel: {error}"))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|error| format!("Unable to request SFTP subsystem: {error}"))?;
    SftpSession::new(channel.into_stream())
        .await
        .map_err(|error| format!("Unable to start SFTP session: {error}"))
}

pub async fn read_dir(handle: &SshHandle, path: &str) -> Result<Vec<RemoteFileEntry>, String> {
    let sftp = open_sftp(handle).await?;
    let remote_path = normalize_remote_path(path);
    let read_dir = sftp
        .read_dir(&remote_path)
        .await
        .map_err(|error| format!("Unable to read remote directory: {error}"))?;

    let mut mapped = Vec::new();
    for entry in read_dir {
        let name = entry.file_name();
        let metadata = entry.metadata();
        let file_type = metadata.file_type();
        let kind = if file_type.is_dir() {
            "directory"
        } else if file_type.is_symlink() {
            "link"
        } else {
            "file"
        };

        mapped.push(RemoteFileEntry {
            id: Uuid::new_v4().to_string(),
            name: name.clone(),
            path: entry.path(),
            kind: kind.to_string(),
            size: metadata.size.unwrap_or(0) as i64,
            modified_at: metadata
                .mtime
                .and_then(|mtime| chrono::DateTime::from_timestamp(mtime as i64, 0))
                .map(|value| value.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "-".to_string()),
            permissions: metadata
                .permissions
                .map(|perm| format!("{:o}", perm & 0o7777))
                .unwrap_or_else(|| "-".to_string()),
            owner: match (metadata.uid, metadata.gid) {
                (Some(uid), Some(gid)) => format!("{uid}:{gid}"),
                (Some(uid), None) => uid.to_string(),
                _ => "-".to_string(),
            },
        });
    }

    mapped.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(mapped)
}

pub async fn create_dir(handle: &SshHandle, path: &str) -> Result<(), String> {
    let result = async {
        let sftp = open_sftp(handle).await?;
        sftp.create_dir(path)
            .await
            .map_err(|error| format!("Unable to create remote directory: {error}"))
    }
    .await;

    disconnect(handle).await;
    result
}

pub async fn create_file(handle: &SshHandle, path: &str) -> Result<(), String> {
    let result = async {
        let sftp = open_sftp(handle).await?;
        let mut file = sftp
            .create(path)
            .await
            .map_err(|error| format!("Unable to create remote file: {error}"))?;
        let _ = file.shutdown().await;
        Ok(())
    }
    .await;

    disconnect(handle).await;
    result
}

pub async fn delete_path_recursive(handle: &SshHandle, path: &str) -> Result<(), String> {
    let result = async {
        let sftp = open_sftp(handle).await?;
        let sftp_result = delete_path_with_sftp(&sftp, path).await;
        sftp_result
    }
    .await;

    result
}

pub async fn rename_path(handle: &SshHandle, from: &str, to: &str) -> Result<(), String> {
    let result = async {
        let sftp = open_sftp(handle).await?;
        sftp.rename(from, to)
            .await
            .map_err(|error| format!("Unable to rename remote path: {error}"))
    }
    .await;

    disconnect(handle).await;
    result
}

pub async fn download_file(
    handle: &SshHandle,
    remote_path: &str,
    local_path: &str,
) -> Result<(), String> {
    download_file_with_progress(
        handle,
        remote_path,
        local_path,
        &AtomicBool::new(false),
        |_, _, _| {},
    )
    .await
}

pub async fn read_file_prefix(
    handle: &SshHandle,
    remote_path: &str,
    max_bytes: usize,
) -> Result<(Vec<u8>, u64, bool), String> {
    let result = async {
        let sftp = open_sftp(handle).await?;
        let total = sftp
            .metadata(remote_path)
            .await
            .ok()
            .and_then(|attr| attr.size)
            .unwrap_or(0);
        let mut remote = sftp
            .open(remote_path)
            .await
            .map_err(|error| format!("Unable to open remote file: {error}"))?;
        let mut buffer = vec![0u8; max_bytes.max(1)];
        let read = remote
            .read(&mut buffer)
            .await
            .map_err(|error| format!("Unable to read remote file: {error}"))?;
        buffer.truncate(read);
        Ok((buffer, total, total > read as u64))
    }
    .await;

    disconnect(handle).await;
    result
}

pub async fn upload_file(
    handle: &SshHandle,
    local_path: &str,
    remote_path: &str,
) -> Result<(), String> {
    upload_file_with_progress(
        handle,
        local_path,
        remote_path,
        &AtomicBool::new(false),
        |_, _, _| {},
    )
    .await
}

pub async fn write_file_atomic(
    handle: &SshHandle,
    remote_path: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let result = async {
        let sftp = open_sftp(handle).await?;
        let parent = remote_parent(remote_path);
        let name = remote_name(remote_path);
        let temp_path = join_remote_path(&parent, &format!(".{name}.alax-edit-{}", Uuid::new_v4()));
        let backup_path =
            join_remote_path(&parent, &format!(".{name}.alax-backup-{}", Uuid::new_v4()));
        let existing = sftp.metadata(remote_path).await.ok();

        let write_result = async {
            let mut remote = sftp
                .create(&temp_path)
                .await
                .map_err(|error| format!("Unable to create temporary remote file: {error}"))?;
            remote
                .write_all(bytes)
                .await
                .map_err(|error| format!("Unable to write temporary remote file: {error}"))?;
            remote
                .flush()
                .await
                .map_err(|error| format!("Unable to flush remote file: {error}"))?;
            remote
                .shutdown()
                .await
                .map_err(|error| format!("Unable to close remote file: {error}"))?;

            let had_existing = existing.is_some();
            if had_existing {
                sftp.rename(remote_path, &backup_path)
                    .await
                    .map_err(|error| format!("Unable to preserve existing remote file: {error}"))?;
            }

            if let Err(error) = sftp.rename(&temp_path, remote_path).await {
                if had_existing {
                    let _ = sftp.rename(&backup_path, remote_path).await;
                }
                return Err(format!("Unable to replace remote file: {error}"));
            }

            if let Some(metadata) = existing {
                let mut attributes = russh_sftp::protocol::FileAttributes::empty();
                attributes.permissions = metadata.permissions;
                let _ = sftp.set_metadata(remote_path, attributes).await;
                let _ = sftp.remove_file(&backup_path).await;
            }
            Ok(())
        }
        .await;

        if write_result.is_err() {
            let _ = sftp.remove_file(&temp_path).await;
        }
        write_result
    }
    .await;

    disconnect(handle).await;
    result
}

pub async fn download_file_with_progress<F>(
    handle: &SshHandle,
    remote_path: &str,
    local_path: &str,
    cancel: &AtomicBool,
    mut progress: F,
) -> Result<(), String>
where
    F: FnMut(u64, u64, u64),
{
    let result = async {
        let sftp = open_sftp(handle).await?;
        let metadata = sftp
            .metadata(remote_path)
            .await
            .map_err(|error| format!("Unable to inspect remote file: {error}"))?;
        let total = metadata.size.unwrap_or(0);
        let signature = metadata.mtime.unwrap_or(0);
        let part_path = local_part_path(Path::new(local_path), total, signature);
        if let Some(parent) = part_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| format!("Unable to create local directory: {error}"))?;
        }
        let mut offset = tokio::fs::metadata(&part_path)
            .await
            .map(|value| value.len())
            .unwrap_or(0);
        if offset > total {
            tokio::fs::remove_file(&part_path)
                .await
                .map_err(|error| format!("Unable to reset invalid partial download: {error}"))?;
            offset = 0;
        }
        let mut remote = sftp
            .open(remote_path)
            .await
            .map_err(|error| format!("Unable to open remote file: {error}"))?;
        remote
            .seek(SeekFrom::Start(offset))
            .await
            .map_err(|error| format!("Unable to resume remote read: {error}"))?;
        let mut local = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&part_path)
            .await
            .map_err(|error| format!("Unable to open partial local file: {error}"))?;
        let mut buffer = vec![0u8; 64 * 1024];
        let mut transferred = offset;
        progress(transferred, total, 0);

        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("Transfer cancelled".to_string());
            }

            let read = remote
                .read(&mut buffer)
                .await
                .map_err(|error| format!("Unable to read remote file: {error}"))?;
            if read == 0 {
                break;
            }
            local
                .write_all(&buffer[..read])
                .await
                .map_err(|error| format!("Unable to write local file: {error}"))?;
            transferred += read as u64;
            progress(transferred, total, read as u64);
        }

        local
            .flush()
            .await
            .map_err(|error| format!("Unable to flush local file: {error}"))?;
        drop(local);
        if transferred != total {
            return Err(format!(
                "Download ended early: received {transferred} of {total} bytes"
            ));
        }
        replace_local_file(&part_path, Path::new(local_path)).await?;
        progress(transferred, total, 0);
        Ok(())
    }
    .await;

    disconnect(handle).await;
    result
}

pub async fn upload_file_with_progress<F>(
    handle: &SshHandle,
    local_path: &str,
    remote_path: &str,
    cancel: &AtomicBool,
    mut progress: F,
) -> Result<(), String>
where
    F: FnMut(u64, u64, u64),
{
    let result = async {
        let sftp = open_sftp(handle).await?;
        let local_metadata = tokio::fs::metadata(local_path)
            .await
            .map_err(|error| format!("Unable to inspect local file: {error}"))?;
        let total = local_metadata.len();
        let modified = local_metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|value| value.as_secs())
            .unwrap_or(0);
        let part_path = remote_part_path(remote_path, total, modified);
        let mut offset = sftp
            .metadata(&part_path)
            .await
            .ok()
            .and_then(|attr| attr.size)
            .unwrap_or(0);
        if offset > total {
            sftp.remove_file(&part_path)
                .await
                .map_err(|error| format!("Unable to reset invalid partial upload: {error}"))?;
            offset = 0;
        }
        let mut local = tokio::fs::File::open(local_path)
            .await
            .map_err(|error| format!("Unable to open local file: {error}"))?;
        local
            .seek(SeekFrom::Start(offset))
            .await
            .map_err(|error| format!("Unable to resume local read: {error}"))?;
        let mut remote = sftp
            .open_with_flags(&part_path, OpenFlags::CREATE | OpenFlags::WRITE)
            .await
            .map_err(|error| format!("Unable to open partial remote file: {error}"))?;
        remote
            .seek(SeekFrom::Start(offset))
            .await
            .map_err(|error| format!("Unable to resume remote write: {error}"))?;
        let mut buffer = vec![0u8; 64 * 1024];
        let mut transferred = offset;
        progress(transferred, total, 0);

        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("Transfer cancelled".to_string());
            }

            let read = local
                .read(&mut buffer)
                .await
                .map_err(|error| format!("Unable to read local file: {error}"))?;
            if read == 0 {
                break;
            }
            remote
                .write_all(&buffer[..read])
                .await
                .map_err(|error| format!("Unable to write remote file: {error}"))?;
            transferred += read as u64;
            progress(transferred, total, read as u64);
        }

        let _ = remote.flush().await;
        let _ = remote.shutdown().await;
        if transferred != total {
            return Err(format!(
                "Upload ended early: sent {transferred} of {total} bytes"
            ));
        }
        let existing = sftp.metadata(remote_path).await.ok();
        replace_remote_file(&sftp, &part_path, remote_path, existing).await?;
        progress(transferred, total, 0);
        Ok(())
    }
    .await;

    disconnect(handle).await;
    result
}

fn delete_path_with_sftp<'a>(
    sftp: &'a SftpSession,
    path: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
    Box::pin(async move {
        match sftp.remove_file(path).await {
            Ok(()) => return Ok(()),
            Err(SftpError::Status(status)) if status.status_code == StatusCode::NoSuchFile => {
                return Ok(());
            }
            Err(_) => {}
        }

        let read_dir = match sftp.read_dir(path).await {
            Ok(dir) => dir,
            Err(SftpError::Status(status)) if status.status_code == StatusCode::NoSuchFile => {
                return Ok(());
            }
            Err(error) => {
                return Err(format!(
                    "Unable to read remote directory before delete: {error}"
                ));
            }
        };

        for entry in read_dir {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            delete_path_with_sftp(sftp, &entry.path()).await?;
        }
        sftp.remove_dir(path)
            .await
            .map_err(|error| format!("Unable to delete remote path: {error}"))
    })
}

async fn replace_remote_file(
    sftp: &SftpSession,
    source: &str,
    destination: &str,
    existing: Option<russh_sftp::protocol::FileAttributes>,
) -> Result<(), String> {
    let parent = remote_parent(destination);
    let name = remote_name(destination);
    let backup = join_remote_path(&parent, &format!(".{name}.alax-backup-{}", Uuid::new_v4()));
    let had_existing = existing.is_some();
    if had_existing {
        sftp.rename(destination, &backup)
            .await
            .map_err(|error| format!("Unable to preserve existing remote file: {error}"))?;
    }

    if let Err(error) = sftp.rename(source, destination).await {
        if had_existing {
            let _ = sftp.rename(&backup, destination).await;
        }
        return Err(format!("Unable to replace remote file: {error}"));
    }

    if let Some(metadata) = existing {
        let mut attributes = russh_sftp::protocol::FileAttributes::empty();
        attributes.permissions = metadata.permissions;
        let _ = sftp.set_metadata(destination, attributes).await;
        let _ = sftp.remove_file(&backup).await;
    }
    Ok(())
}

async fn replace_local_file(source: &Path, destination: &Path) -> Result<(), String> {
    let backup = append_local_suffix(destination, &format!(".alax-backup-{}", Uuid::new_v4()));
    let had_existing = tokio::fs::try_exists(destination)
        .await
        .map_err(|error| format!("Unable to inspect local destination: {error}"))?;
    if had_existing {
        tokio::fs::rename(destination, &backup)
            .await
            .map_err(|error| format!("Unable to preserve existing local file: {error}"))?;
    }
    if let Err(error) = tokio::fs::rename(source, destination).await {
        if had_existing {
            let _ = tokio::fs::rename(&backup, destination).await;
        }
        return Err(format!("Unable to replace local file: {error}"));
    }
    if had_existing {
        let _ = tokio::fs::remove_file(backup).await;
    }
    Ok(())
}

fn remote_part_path(destination: &str, size: u64, modified: u64) -> String {
    let parent = remote_parent(destination);
    let name = remote_name(destination);
    join_remote_path(&parent, &format!(".{name}.alax-part-{size}-{modified}"))
}

fn local_part_path(destination: &Path, size: u64, modified: u32) -> PathBuf {
    append_local_suffix(destination, &format!(".alax-part-{size}-{modified}"))
}

fn append_local_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn normalize_remote_path(path: &str) -> String {
    if path.trim().is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    }
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
        .unwrap_or("file")
        .to_string()
}

fn join_remote_path(parent: &str, name: &str) -> String {
    if parent == "/" {
        format!("/{name}")
    } else {
        format!("{}/{name}", parent.trim_end_matches('/'))
    }
}
