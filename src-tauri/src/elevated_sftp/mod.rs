use crate::{credentials, db::ServerConnectionConfig, sftp, ssh};
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use uuid::Uuid;

pub fn should_try_elevation(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("permission")
        || lower.contains("denied")
        || lower.contains("access")
        || lower.contains("failure")
        || lower.contains("read-only")
}

pub async fn read_dir(
    config: &ServerConnectionConfig,
    handle: &ssh::SshHandle,
    path: &str,
) -> Result<Vec<sftp::RemoteFileEntry>, String> {
    let script = r#"
import json
import os
import stat
import sys
import time

base = sys.argv[1] if len(sys.argv) > 1 and sys.argv[1].strip() else "/"
rows = []
for name in os.listdir(base):
    path = os.path.join(base, name) if base != "/" else "/" + name
    st = os.lstat(path)
    mode = st.st_mode
    if stat.S_ISDIR(mode):
        kind = "directory"
    elif stat.S_ISLNK(mode):
        try:
            kind = "directory" if stat.S_ISDIR(os.stat(path).st_mode) else "link"
        except OSError:
            kind = "link"
    else:
        kind = "file"
    rows.append({
        "id": path,
        "name": name,
        "path": path,
        "kind": kind,
        "size": int(st.st_size),
        "modifiedAt": time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(st.st_mtime)),
        "permissions": format(stat.S_IMODE(mode), "o"),
        "owner": f"{st.st_uid}:{st.st_gid}",
    })
rows.sort(key=lambda item: (item["kind"], item["name"].lower()))
print(json.dumps(rows, ensure_ascii=False))
"#;
    let command = format!(
        "sudo -S -p '' -- python3 -c {} {}",
        shell_quote(script),
        shell_quote(path)
    );
    let output = run_sudo_command_on_handle(config, handle, &command).await?;
    if !output.is_success() {
        return Err(format!(
            "Elevated read directory failed with status {}: {}",
            output.exit_status.unwrap_or(255),
            output.combined_output().trim()
        ));
    }
    serde_json::from_str(&output.stdout).map_err(|error| {
        format!(
            "Unable to parse elevated directory listing: {error}; output: {}",
            output.stdout.trim()
        )
    })
}

pub async fn create_dir(config: &ServerConnectionConfig, path: &str) -> Result<(), String> {
    sudo_exec(
        config,
        &format!("sudo -S -p '' -- mkdir -p -- {}", shell_quote(path)),
        "create directory",
    )
    .await
}

pub async fn create_file(config: &ServerConnectionConfig, path: &str) -> Result<(), String> {
    sudo_exec(
        config,
        &format!("sudo -S -p '' -- touch -- {}", shell_quote(path)),
        "create file",
    )
    .await
}

pub async fn delete_path(config: &ServerConnectionConfig, path: &str) -> Result<(), String> {
    validate_destructive_path(path)?;
    sudo_exec(
        config,
        &format!("sudo -S -p '' -- rm -rf -- {}", shell_quote(path)),
        "delete path",
    )
    .await
}

pub async fn remote_path_exists(
    config: &ServerConnectionConfig,
    path: &str,
) -> Result<bool, String> {
    let command = format!(
        "sh -c {} sh {}",
        shell_quote("test -e \"$1\""),
        shell_quote(path)
    );
    let output = ssh::run_command_with_input(config, &command, None).await?;
    match output.exit_status.unwrap_or(255) {
        0 => Ok(true),
        1 => {
            let command = format!("sudo -S -p '' -- test -e {}", shell_quote(path));
            match run_sudo_command(config, &command).await {
                Ok(output) => match output.exit_status.unwrap_or(255) {
                    0 => Ok(true),
                    1 => Ok(false),
                    status => Err(format!(
                        "Unable to verify remote path existence with sudo, status {status}: {}",
                        output.combined_output().trim()
                    )),
                },
                Err(_) => Ok(false),
            }
        }
        status => Err(format!(
            "Unable to verify remote path existence, status {status}: {}",
            output.combined_output().trim()
        )),
    }
}

pub async fn read_file_base64(
    config: &ServerConnectionConfig,
    path: &str,
    max_bytes: usize,
) -> Result<(String, u64, bool), String> {
    let script = r#"
import base64
import os
import sys

path = sys.argv[1]
limit = int(sys.argv[2])
total = os.path.getsize(path)
with open(path, "rb") as fh:
    data = fh.read(limit)
print(f"{total}|{1 if total > len(data) else 0}|{base64.b64encode(data).decode('ascii')}")
"#;
    let command = format!(
        "sudo -S -p '' -- python3 -c {} {} {}",
        shell_quote(script),
        shell_quote(path),
        max_bytes
    );
    let output = run_sudo_command(config, &command).await?;
    if !output.is_success() {
        return Err(format!(
            "Elevated preview read failed with status {}: {}",
            output.exit_status.unwrap_or(255),
            output.combined_output().trim()
        ));
    }
    let line = output.stdout.trim();
    let mut parts = line.splitn(3, '|');
    let total = parts.next().unwrap_or("0").parse().unwrap_or(0);
    let truncated = parts.next().unwrap_or("0") == "1";
    let data = parts.next().unwrap_or("").to_string();
    Ok((data, total, truncated))
}

pub async fn rename_path(
    config: &ServerConnectionConfig,
    from: &str,
    to: &str,
) -> Result<(), String> {
    sudo_exec(
        config,
        &format!(
            "sudo -S -p '' -- mv -f -- {} {}",
            shell_quote(from),
            shell_quote(to)
        ),
        "rename path",
    )
    .await
}

pub async fn compress_paths(
    config: &ServerConnectionConfig,
    paths: &[String],
    destination: &str,
) -> Result<(), String> {
    if paths.is_empty() {
        return Err("No remote paths selected for compression".to_string());
    }
    let parent = remote_parent(destination);
    if paths.iter().any(|path| remote_parent(path) != parent) {
        return Err("All compressed paths must be in the destination directory".to_string());
    }
    let script = "cd \"$1\" && shift && tar -czf \"$1\" -- \"$@\"";
    let names: Vec<String> = paths.iter().map(|path| remote_name(path)).collect();
    let mut command = format!(
        "sudo -S -p '' -- sh -c {} sh {} {}",
        shell_quote(script),
        shell_quote(&parent),
        shell_quote(destination)
    );
    for name in names {
        command.push(' ');
        command.push_str(&shell_quote(&name));
    }
    sudo_exec(config, &command, "compress paths").await
}

pub async fn set_permissions(
    config: &ServerConnectionConfig,
    paths: &[String],
    mode: &str,
    recursive: bool,
) -> Result<(), String> {
    if paths.is_empty() {
        return Err("No remote paths selected".to_string());
    }
    let recursive_flag = if recursive { "-R " } else { "" };
    let mut command = format!("sudo -S -p '' -- chmod {recursive_flag}{mode} --");
    for path in paths {
        command.push(' ');
        command.push_str(&shell_quote(path));
    }
    sudo_exec(config, &command, "change permissions").await
}

pub async fn checksum(
    config: &ServerConnectionConfig,
    path: &str,
    program: &str,
) -> Result<String, String> {
    let command = format!("sudo -S -p '' -- {program} -- {}", shell_quote(path));
    let output = run_sudo_command(config, &command).await?;
    if !output.is_success() {
        return Err(format!(
            "Elevated checksum failed with status {}: {}",
            output.exit_status.unwrap_or(255),
            output.combined_output().trim()
        ));
    }
    Ok(output.stdout)
}

pub async fn extract_uploaded_directory(
    config: &ServerConnectionConfig,
    archive_path: &str,
    destination_parent: &str,
) -> Result<(), String> {
    let script = "mkdir -p -- \"$1\" && tar -xzf \"$2\" -C \"$1\"; status=$?; rm -f -- \"$2\"; exit \"$status\"";
    sudo_exec_with_timeout(
        config,
        &format!(
            "sudo -S -p '' -- sh -c {} sh {} {}",
            shell_quote(script),
            shell_quote(destination_parent),
            shell_quote(archive_path)
        ),
        "extract uploaded directory",
        Duration::from_secs(30 * 60),
    )
    .await
}

pub async fn prepare_directory_download(
    config: &ServerConnectionConfig,
    remote_path: &str,
    archive_path: &str,
) -> Result<(), String> {
    let parent = remote_parent(remote_path);
    let name = remote_name(remote_path);
    let script =
        "tar -czf \"$1\" -C \"$2\" -- \"$3\" && chown -- \"$4\" \"$1\" && chmod 600 -- \"$1\"";
    sudo_exec_with_timeout(
        config,
        &format!(
            "sudo -S -p '' -- sh -c {} sh {} {} {} {}",
            shell_quote(script),
            shell_quote(archive_path),
            shell_quote(&parent),
            shell_quote(&name),
            shell_quote(&config.username)
        ),
        "prepare directory download",
        Duration::from_secs(30 * 60),
    )
    .await
}

pub async fn upload_file_with_progress<F>(
    config: &ServerConnectionConfig,
    local_path: &str,
    remote_path: &str,
    cancel: &AtomicBool,
    progress: F,
) -> Result<(), String>
where
    F: FnMut(u64, u64, u64),
{
    let temp_path = format!("/tmp/alax-ssh-manager-upload-{}", Uuid::new_v4());
    let handle = ssh::connect(config).await?;
    sftp::upload_file_with_progress(&handle, local_path, &temp_path, cancel, progress).await?;

    let parent = remote_parent(remote_path);
    let part_path = format!("{}.alax-part-{}", remote_path, Uuid::new_v4());
    let script = "mkdir -p -- \"$1\" && cp -f -- \"$2\" \"$4\" && if [ -e \"$3\" ]; then chown --reference=\"$3\" \"$4\" && chmod --reference=\"$3\" \"$4\"; fi && mv -f -- \"$4\" \"$3\"; status=$?; rm -f -- \"$2\" \"$4\"; exit \"$status\"";
    sudo_exec(
        config,
        &format!(
            "sudo -S -p '' -- sh -c {} sh {} {} {} {}",
            shell_quote(script),
            shell_quote(&parent),
            shell_quote(&temp_path),
            shell_quote(remote_path),
            shell_quote(&part_path)
        ),
        "upload file",
    )
    .await
}

pub async fn download_file_with_progress<F>(
    config: &ServerConnectionConfig,
    remote_path: &str,
    local_path: &str,
    cancel: &AtomicBool,
    progress: F,
) -> Result<(), String>
where
    F: FnMut(u64, u64, u64),
{
    let temp_path = format!("/tmp/alax-ssh-manager-download-{}", Uuid::new_v4());
    let prepare_script = "cp -f -- \"$1\" \"$2\" && chown -- \"$3\" \"$2\" && chmod 600 -- \"$2\"";
    sudo_exec(
        config,
        &format!(
            "sudo -S -p '' -- sh -c {} sh {} {} {}",
            shell_quote(prepare_script),
            shell_quote(remote_path),
            shell_quote(&temp_path),
            shell_quote(&config.username)
        ),
        "prepare download",
    )
    .await?;

    let handle = ssh::connect(config).await?;
    let result =
        sftp::download_file_with_progress(&handle, &temp_path, local_path, cancel, progress).await;
    let _ = sudo_exec(
        config,
        &format!("sudo -S -p '' -- rm -f -- {}", shell_quote(&temp_path)),
        "cleanup download",
    )
    .await;
    result
}

async fn sudo_exec(
    config: &ServerConnectionConfig,
    command: &str,
    action: &str,
) -> Result<(), String> {
    let output = run_sudo_command(config, command).await?;
    if output.is_success() {
        Ok(())
    } else {
        let message = output.combined_output();
        Err(format!(
            "Elevated {action} failed with status {}: {}",
            output.exit_status.unwrap_or(255),
            message.trim()
        ))
    }
}

async fn sudo_exec_with_timeout(
    config: &ServerConnectionConfig,
    command: &str,
    action: &str,
    command_timeout: Duration,
) -> Result<(), String> {
    let password = sudo_password(config).await?;
    let input = format!("{password}\n");
    let output =
        ssh::run_command_with_input_timeout(config, command, Some(&input), command_timeout).await?;
    if output.is_success() {
        Ok(())
    } else {
        Err(format!(
            "Elevated {action} failed with status {}: {}",
            output.exit_status.unwrap_or(255),
            output.combined_output().trim()
        ))
    }
}

async fn run_sudo_command(
    config: &ServerConnectionConfig,
    command: &str,
) -> Result<ssh::CommandResult, String> {
    let password = sudo_password(config).await?;
    let input = format!("{password}\n");
    ssh::run_command_with_input(config, command, Some(&input)).await
}

async fn run_sudo_command_on_handle(
    config: &ServerConnectionConfig,
    handle: &ssh::SshHandle,
    command: &str,
) -> Result<ssh::CommandResult, String> {
    let password = sudo_password(config).await?;
    let input = format!("{password}\n");
    ssh::run_command_on_handle_with_input_timeout(
        handle,
        command,
        Some(&input),
        Duration::from_secs(60),
    )
    .await
}

async fn sudo_password(config: &ServerConnectionConfig) -> Result<String, String> {
    if config.auth_type != "password" {
        return Err("该路径需要管理员权限；当前连接没有可用于 sudo 的密码凭据，请使用密码登录或改用 root 账号。".to_string());
    }

    let reference = config
        .credential_ref
        .as_ref()
        .ok_or_else(|| "该路径需要管理员权限，但没有找到可用于 sudo 的密码凭据。".to_string())?;
    credentials::read_secret_async(reference.clone())
        .await
        .map_err(|error| format!("无法从系统凭据读取 sudo 密码: {error}"))
}

fn validate_destructive_path(path: &str) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" || trimmed == "." || trimmed == ".." {
        return Err("Refusing to delete an unsafe remote path".to_string());
    }
    Ok(())
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

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use super::{remote_parent, shell_quote};

    #[test]
    fn quotes_shell_arguments() {
        assert_eq!(shell_quote("/tmp/a b"), "'/tmp/a b'");
        assert_eq!(shell_quote("/tmp/a'b"), "'/tmp/a'\"'\"'b'");
    }

    #[test]
    fn finds_remote_parent() {
        assert_eq!(remote_parent("/opt/app/file.txt"), "/opt/app");
        assert_eq!(remote_parent("/file.txt"), "/");
        assert_eq!(remote_parent("file.txt"), ".");
    }
}
