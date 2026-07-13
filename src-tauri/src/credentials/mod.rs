use uuid::Uuid;

const SERVICE_NAME: &str = "com.alax.ssh-manager";
const LEGACY_SERVICE_NAME: &str = "ALAX SSH Manager";
const WINDOWS_TARGET_PREFIX: &str = "ALAX SSH Manager:";

pub fn store_name() -> &'static str {
    SERVICE_NAME
}

pub fn create_secret_ref(server_id: &str, kind: &str) -> String {
    format!("alax:{}:{}:{}", server_id, kind, Uuid::new_v4())
}

pub fn save_secret(reference: &str, secret: &str) -> Result<(), String> {
    platform_save_secret(reference, secret)
}

pub fn read_secret(reference: &str) -> Result<String, String> {
    platform_read_secret(reference)
}

pub fn delete_secret(reference: &str) -> Result<(), String> {
    platform_delete_secret(reference)
}

pub async fn read_secret_async(reference: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || read_secret(&reference))
        .await
        .map_err(|error| format!("Credential task panicked: {error}"))?
        .map_err(|error| {
            format!(
                "凭据未找到（可能已被系统清理），请在服务器列表中编辑该服务器并重新输入密码/私钥。\n\n详细信息: {error}"
            )
        })
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::{create_secret_ref, delete_secret, read_secret, save_secret};

    #[test]
    fn secure_store_round_trips_empty_secret() {
        let reference = create_secret_ref("credential-test", "empty-password");
        save_secret(&reference, "").expect("empty secret should be stored");
        assert_eq!(
            read_secret(&reference).expect("empty secret should be read"),
            ""
        );
        delete_secret(&reference).expect("test credential should be deleted");
    }
}

#[cfg(target_os = "windows")]
fn platform_save_secret(reference: &str, secret: &str) -> Result<(), String> {
    windows_store::save_secret(reference, secret)
}

#[cfg(target_os = "windows")]
fn platform_read_secret(reference: &str) -> Result<String, String> {
    windows_store::read_secret(reference)
}

#[cfg(target_os = "windows")]
fn platform_delete_secret(reference: &str) -> Result<(), String> {
    windows_store::delete_secret(reference)
}

#[cfg(not(target_os = "windows"))]
fn platform_save_secret(reference: &str, secret: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE_NAME, reference).map_err(|error| error.to_string())?;
    entry
        .set_password(secret)
        .map_err(|error| error.to_string())?;
    let actual = entry.get_password().map_err(|error| error.to_string())?;
    if actual == secret {
        Ok(())
    } else {
        Err("secure storage verification failed".to_string())
    }
}

#[cfg(not(target_os = "windows"))]
fn platform_read_secret(reference: &str) -> Result<String, String> {
    keyring::Entry::new(SERVICE_NAME, reference)
        .map_err(|error| error.to_string())?
        .get_password()
        .map_err(|error| error.to_string())
}

#[cfg(not(target_os = "windows"))]
fn platform_delete_secret(reference: &str) -> Result<(), String> {
    match keyring::Entry::new(SERVICE_NAME, reference).and_then(|entry| entry.delete_credential()) {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(target_os = "windows")]
mod windows_store {
    use super::{LEGACY_SERVICE_NAME, SERVICE_NAME, WINDOWS_TARGET_PREFIX};
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };

    pub fn save_secret(reference: &str, secret: &str) -> Result<(), String> {
        let target = target_name(reference);
        let mut target_w = wide_null(&target);
        let mut username_w = wide_null(SERVICE_NAME);
        let mut blob = utf16le_bytes(secret);

        let credential = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: target_w.as_mut_ptr(),
            Comment: null_mut(),
            LastWritten: Default::default(),
            CredentialBlobSize: u32::try_from(blob.len())
                .map_err(|_| "凭据内容过大，无法保存到 Windows 凭据管理器。".to_string())?,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: null_mut(),
            TargetAlias: null_mut(),
            UserName: username_w.as_mut_ptr(),
        };

        let ok = unsafe { CredWriteW(&credential, 0) };
        if ok == 0 {
            return Err(format_windows_error("写入 Windows 凭据管理器失败"));
        }

        let actual = read_secret(reference)?;
        if actual == secret {
            Ok(())
        } else {
            Err("Windows 凭据写入后校验失败。".to_string())
        }
    }

    pub fn read_secret(reference: &str) -> Result<String, String> {
        let mut last_error = None;
        for target in candidate_targets(reference) {
            match read_target(&target) {
                Ok(secret) => return Ok(secret),
                Err(error) => last_error = Some(error),
            }
        }

        Err(last_error.unwrap_or_else(|| "Windows 凭据管理器中没有找到对应条目。".to_string()))
    }

    pub fn delete_secret(reference: &str) -> Result<(), String> {
        for target in candidate_targets(reference) {
            let mut target_w = wide_null(&target);
            unsafe {
                CredDeleteW(target_w.as_mut_ptr(), CRED_TYPE_GENERIC, 0);
            }
        }
        Ok(())
    }

    fn read_target(target: &str) -> Result<String, String> {
        let mut target_w = wide_null(target);
        let mut credential_ptr = null_mut();
        let ok = unsafe {
            CredReadW(
                target_w.as_mut_ptr(),
                CRED_TYPE_GENERIC,
                0,
                &mut credential_ptr,
            )
        };

        if ok == 0 {
            return Err(format_windows_error("读取 Windows 凭据管理器失败"));
        }

        if credential_ptr.is_null() {
            return Err("Windows 凭据管理器返回空凭据。".to_string());
        }

        let result = unsafe {
            let credential = &*credential_ptr;
            if credential.CredentialBlobSize == 0 {
                CredFree(credential_ptr.cast());
                return Ok(String::new());
            }
            let bytes = std::slice::from_raw_parts(
                credential.CredentialBlob,
                credential.CredentialBlobSize as usize,
            );
            let secret = utf16le_string(bytes);
            CredFree(credential_ptr.cast());
            secret
        };

        result
    }

    fn target_name(reference: &str) -> String {
        format!("{WINDOWS_TARGET_PREFIX}{reference}")
    }

    fn candidate_targets(reference: &str) -> Vec<String> {
        vec![
            target_name(reference),
            reference.to_string(),
            format!("{reference}.{SERVICE_NAME}"),
            format!("{reference}.{LEGACY_SERVICE_NAME}"),
        ]
    }

    fn wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn utf16le_bytes(value: &str) -> Vec<u8> {
        value.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    fn utf16le_string(bytes: &[u8]) -> Result<String, String> {
        if !bytes.len().is_multiple_of(2) {
            return Err("Windows 凭据内容不是有效的 UTF-16。".to_string());
        }

        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        String::from_utf16(&units).map_err(|error| format!("解析 Windows 凭据失败：{error}"))
    }

    fn format_windows_error(context: &str) -> String {
        let code = unsafe { GetLastError() };
        format!("{context}，Windows 错误码：{code}")
    }
}
