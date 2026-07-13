mod schema;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Result as SqlResult};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerGroup {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub sort_order: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerProfile {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: i64,
    pub username: String,
    pub auth_type: String,
    pub group_id: Option<String>,
    pub tags: Vec<String>,
    pub note: String,
    pub status: String,
    pub last_connected_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ServerConnectionConfig {
    pub id: String,
    pub host: String,
    pub port: i64,
    pub username: String,
    pub auth_type: String,
    pub credential_ref: Option<String>,
    pub private_key_ref: Option<String>,
    pub private_key_path: Option<String>,
    pub trusted_host_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedHostKey {
    pub server_id: String,
    pub algorithm: String,
    pub fingerprint: String,
    pub trusted_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelRule {
    pub id: String,
    pub server_id: String,
    pub name: String,
    pub local_host: String,
    pub local_port: i64,
    pub remote_host: String,
    pub remote_port: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelRuleInput {
    pub id: Option<String>,
    pub server_id: String,
    pub name: String,
    pub local_host: String,
    pub local_port: i64,
    pub remote_host: String,
    pub remote_port: i64,
}

#[derive(Debug, Clone)]
pub struct ServerSecretState {
    pub auth_type: String,
    pub credential_ref: Option<String>,
    pub private_key_ref: Option<String>,
    pub private_key_path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupInput {
    pub id: Option<String>,
    pub name: String,
    pub parent_id: Option<String>,
    pub sort_order: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInput {
    pub id: Option<String>,
    pub name: String,
    pub host: String,
    pub port: i64,
    pub username: String,
    pub auth_type: String,
    pub password: Option<String>,
    #[serde(default)]
    pub use_empty_password: bool,
    pub private_key_path: Option<String>,
    pub private_key_content: Option<String>,
    pub passphrase: Option<String>,
    pub group_id: Option<String>,
    pub tags: Vec<String>,
    pub note: String,
}

pub fn initialize(database_path: &Path) -> SqlResult<()> {
    let connection = open_connection(database_path)?;
    connection.execute_batch(schema::INITIAL_SCHEMA)?;
    connection.execute(
        "UPDATE servers SET status = 'idle' WHERE status IN ('connected', 'available')",
        [],
    )?;
    Ok(())
}

pub fn list_groups(database_path: &Path) -> SqlResult<Vec<ServerGroup>> {
    let connection = open_connection(database_path)?;
    let mut statement = connection.prepare(
        "SELECT id, name, parent_id, sort_order FROM groups ORDER BY sort_order ASC, name ASC",
    )?;

    let rows = statement.query_map([], |row| {
        Ok(ServerGroup {
            id: row.get(0)?,
            name: row.get(1)?,
            parent_id: row.get(2)?,
            sort_order: row.get(3)?,
        })
    })?;

    rows.collect()
}

pub fn list_servers(database_path: &Path) -> SqlResult<Vec<ServerProfile>> {
    let connection = open_connection(database_path)?;
    let mut statement = connection.prepare(
        "SELECT id, name, host, port, username, auth_type, group_id, tags, note, status, last_connected_at
         FROM servers
         ORDER BY name ASC",
    )?;

    let rows = statement.query_map([], |row| {
        let tags_json: String = row.get(7)?;
        let tags = serde_json::from_str(&tags_json).unwrap_or_default();

        Ok(ServerProfile {
            id: row.get(0)?,
            name: row.get(1)?,
            host: row.get(2)?,
            port: row.get(3)?,
            username: row.get(4)?,
            auth_type: row.get(5)?,
            group_id: row.get(6)?,
            tags,
            note: row.get(8)?,
            status: row.get(9)?,
            last_connected_at: row.get(10)?,
        })
    })?;

    rows.collect()
}

pub fn create_group(database_path: &Path, input: GroupInput) -> SqlResult<ServerGroup> {
    let connection = open_connection(database_path)?;
    let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let now = now();

    connection.execute(
        "INSERT INTO groups (id, name, parent_id, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![id, input.name, input.parent_id, input.sort_order, now],
    )?;

    get_group(database_path, &id)
}

pub fn update_group(database_path: &Path, input: GroupInput) -> SqlResult<ServerGroup> {
    let connection = open_connection(database_path)?;
    let id = input.id.ok_or_else(|| {
        rusqlite::Error::InvalidParameterName("group id is required for update".to_string())
    })?;

    connection.execute(
        "UPDATE groups
         SET name = ?2, parent_id = ?3, sort_order = ?4, updated_at = ?5
         WHERE id = ?1",
        params![id, input.name, input.parent_id, input.sort_order, now()],
    )?;

    get_group(database_path, &id)
}

pub fn delete_group(database_path: &Path, group_id: &str) -> SqlResult<()> {
    let connection = open_connection(database_path)?;
    connection.execute(
        "UPDATE servers SET group_id = NULL, updated_at = ?2 WHERE group_id = ?1",
        params![group_id, now()],
    )?;
    connection.execute("DELETE FROM groups WHERE id = ?1", params![group_id])?;
    Ok(())
}

pub fn create_server(
    database_path: &Path,
    input: ServerInput,
    credential_ref: Option<String>,
    private_key_ref: Option<String>,
) -> SqlResult<ServerProfile> {
    let mut connection = open_connection(database_path)?;
    let transaction = connection.transaction()?;
    let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let tags = serde_json::to_string(&input.tags).unwrap_or_else(|_| "[]".to_string());
    let now = now();

    transaction.execute(
        "INSERT INTO servers (
            id, name, host, port, username, auth_type, credential_ref, private_key_ref,
            private_key_path, group_id, tags, note, status, created_at, updated_at
          )
          VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'idle', ?13, ?13)",
        params![
            id,
            input.name,
            input.host,
            input.port,
            input.username,
            input.auth_type,
            credential_ref,
            private_key_ref,
            input.private_key_path,
            input.group_id,
            tags,
            input.note,
            now
        ],
    )?;

    let server = get_server_from_connection(&transaction, &id)?;
    transaction.commit()?;
    Ok(server)
}

pub fn update_server(
    database_path: &Path,
    input: ServerInput,
    credential_ref: Option<String>,
    private_key_ref: Option<String>,
) -> SqlResult<ServerProfile> {
    let mut connection = open_connection(database_path)?;
    let transaction = connection.transaction()?;
    let id = input.id.clone().ok_or_else(|| {
        rusqlite::Error::InvalidParameterName("server id is required for update".to_string())
    })?;
    let existing = get_server_secret_state_from_connection(&transaction, &id)?;
    let previous_endpoint: (String, i64) = transaction.query_row(
        "SELECT host, port FROM servers WHERE id = ?1",
        params![id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let tags = serde_json::to_string(&input.tags).unwrap_or_else(|_| "[]".to_string());
    let has_new_credential = credential_ref.is_some();
    let has_new_private_key = private_key_ref.is_some();
    let has_private_key_path = input
        .private_key_path
        .as_deref()
        .map(|path| !path.trim().is_empty())
        .unwrap_or(false);
    let preserve_credential = input.auth_type == existing.auth_type
        && matches!(
            input.auth_type.as_str(),
            "password" | "private_key_with_passphrase"
        );
    let preserve_private_key = matches!(
        input.auth_type.as_str(),
        "private_key" | "private_key_with_passphrase"
    ) && matches!(
        existing.auth_type.as_str(),
        "private_key" | "private_key_with_passphrase"
    );
    let next_credential_ref = if has_new_credential {
        credential_ref
    } else if preserve_credential {
        existing.credential_ref
    } else {
        None
    };
    let next_private_key_ref = if has_new_private_key {
        private_key_ref
    } else if has_private_key_path {
        None
    } else if preserve_private_key {
        existing.private_key_ref
    } else {
        None
    };
    let next_private_key_path = if has_new_private_key {
        None
    } else if has_private_key_path {
        input.private_key_path
    } else if preserve_private_key {
        existing.private_key_path
    } else {
        None
    };

    transaction.execute(
        "UPDATE servers
         SET name = ?2,
             host = ?3,
             port = ?4,
             username = ?5,
             auth_type = ?6,
             credential_ref = ?7,
             private_key_ref = ?8,
             private_key_path = ?9,
             group_id = ?10,
             tags = ?11,
             note = ?12,
             updated_at = ?13
         WHERE id = ?1",
        params![
            id,
            input.name,
            input.host,
            input.port,
            input.username,
            input.auth_type,
            next_credential_ref,
            next_private_key_ref,
            next_private_key_path,
            input.group_id,
            tags,
            input.note,
            now()
        ],
    )?;

    if previous_endpoint.0 != input.host || previous_endpoint.1 != input.port {
        transaction.execute(
            "DELETE FROM trusted_host_keys WHERE server_id = ?1",
            params![id],
        )?;
    }

    let server = get_server_from_connection(&transaction, &id)?;
    transaction.commit()?;
    Ok(server)
}

pub fn delete_server(
    database_path: &Path,
    server_id: &str,
) -> SqlResult<(Option<String>, Option<String>)> {
    let mut connection = open_connection(database_path)?;
    let transaction = connection.transaction()?;
    let refs = get_server_secret_state_from_connection(&transaction, server_id)?;
    transaction.execute("DELETE FROM servers WHERE id = ?1", params![server_id])?;
    transaction.commit()?;
    Ok((refs.credential_ref, refs.private_key_ref))
}

pub fn get_server(database_path: &Path, server_id: &str) -> SqlResult<ServerProfile> {
    let connection = open_connection(database_path)?;
    get_server_from_connection(&connection, server_id)
}

fn get_server_from_connection(
    connection: &Connection,
    server_id: &str,
) -> SqlResult<ServerProfile> {
    connection.query_row(
        "SELECT id, name, host, port, username, auth_type, group_id, tags, note, status, last_connected_at
         FROM servers
         WHERE id = ?1",
        params![server_id],
        |row| {
            let tags_json: String = row.get(7)?;
            let tags = serde_json::from_str(&tags_json).unwrap_or_default();
            Ok(ServerProfile {
                id: row.get(0)?,
                name: row.get(1)?,
                host: row.get(2)?,
                port: row.get(3)?,
                username: row.get(4)?,
                auth_type: row.get(5)?,
                group_id: row.get(6)?,
                tags,
                note: row.get(8)?,
                status: row.get(9)?,
                last_connected_at: row.get(10)?,
            })
        },
    )
}

pub fn get_server_connection(
    database_path: &Path,
    server_id: &str,
) -> SqlResult<ServerConnectionConfig> {
    let connection = open_connection(database_path)?;
    connection.query_row(
        "SELECT s.id, s.host, s.port, s.username, s.auth_type, s.credential_ref,
                s.private_key_ref, s.private_key_path, h.fingerprint
         FROM servers s
         LEFT JOIN trusted_host_keys h ON h.server_id = s.id
         WHERE s.id = ?1",
        params![server_id],
        |row| {
            Ok(ServerConnectionConfig {
                id: row.get(0)?,
                host: row.get(1)?,
                port: row.get(2)?,
                username: row.get(3)?,
                auth_type: row.get(4)?,
                credential_ref: row.get(5)?,
                private_key_ref: row.get(6)?,
                private_key_path: row.get(7)?,
                trusted_host_key: row.get(8)?,
            })
        },
    )
}

pub fn get_trusted_host_key(
    database_path: &Path,
    server_id: &str,
) -> SqlResult<Option<TrustedHostKey>> {
    let connection = open_connection(database_path)?;
    connection
        .query_row(
            "SELECT server_id, algorithm, fingerprint, trusted_at FROM trusted_host_keys WHERE server_id = ?1",
            params![server_id],
            |row| {
                Ok(TrustedHostKey {
                    server_id: row.get(0)?,
                    algorithm: row.get(1)?,
                    fingerprint: row.get(2)?,
                    trusted_at: row.get(3)?,
                })
            },
        )
        .optional()
}

pub fn trust_host_key(
    database_path: &Path,
    server_id: &str,
    algorithm: &str,
    fingerprint: &str,
) -> SqlResult<TrustedHostKey> {
    let connection = open_connection(database_path)?;
    let trusted_at = now();
    connection.execute(
        "INSERT INTO trusted_host_keys (server_id, algorithm, fingerprint, trusted_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(server_id) DO UPDATE SET
           algorithm = excluded.algorithm,
           fingerprint = excluded.fingerprint,
           trusted_at = excluded.trusted_at",
        params![server_id, algorithm, fingerprint, trusted_at],
    )?;
    get_trusted_host_key(database_path, server_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

pub fn clear_trusted_host_key(database_path: &Path, server_id: &str) -> SqlResult<()> {
    let connection = open_connection(database_path)?;
    connection.execute(
        "DELETE FROM trusted_host_keys WHERE server_id = ?1",
        params![server_id],
    )?;
    Ok(())
}

pub fn list_tunnel_rules(database_path: &Path, server_id: &str) -> SqlResult<Vec<TunnelRule>> {
    let connection = open_connection(database_path)?;
    let mut statement = connection.prepare(
        "SELECT id, server_id, name, local_host, local_port, remote_host, remote_port
         FROM tunnel_rules WHERE server_id = ?1 ORDER BY name ASC",
    )?;
    let rows = statement.query_map(params![server_id], |row| {
        Ok(TunnelRule {
            id: row.get(0)?,
            server_id: row.get(1)?,
            name: row.get(2)?,
            local_host: row.get(3)?,
            local_port: row.get(4)?,
            remote_host: row.get(5)?,
            remote_port: row.get(6)?,
        })
    })?;
    rows.collect()
}

pub fn get_tunnel_rule(database_path: &Path, tunnel_id: &str) -> SqlResult<TunnelRule> {
    let connection = open_connection(database_path)?;
    connection.query_row(
        "SELECT id, server_id, name, local_host, local_port, remote_host, remote_port
         FROM tunnel_rules WHERE id = ?1",
        params![tunnel_id],
        |row| {
            Ok(TunnelRule {
                id: row.get(0)?,
                server_id: row.get(1)?,
                name: row.get(2)?,
                local_host: row.get(3)?,
                local_port: row.get(4)?,
                remote_host: row.get(5)?,
                remote_port: row.get(6)?,
            })
        },
    )
}

pub fn save_tunnel_rule(database_path: &Path, input: TunnelRuleInput) -> SqlResult<TunnelRule> {
    let connection = open_connection(database_path)?;
    let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let timestamp = now();
    connection.execute(
        "INSERT INTO tunnel_rules (
           id, server_id, name, local_host, local_port, remote_host, remote_port, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
         ON CONFLICT(id) DO UPDATE SET
           server_id = excluded.server_id,
           name = excluded.name,
           local_host = excluded.local_host,
           local_port = excluded.local_port,
           remote_host = excluded.remote_host,
           remote_port = excluded.remote_port,
           updated_at = excluded.updated_at",
        params![
            id,
            input.server_id,
            input.name,
            input.local_host,
            input.local_port,
            input.remote_host,
            input.remote_port,
            timestamp
        ],
    )?;
    get_tunnel_rule(database_path, &id)
}

pub fn delete_tunnel_rule(database_path: &Path, tunnel_id: &str) -> SqlResult<()> {
    let connection = open_connection(database_path)?;
    connection.execute("DELETE FROM tunnel_rules WHERE id = ?1", params![tunnel_id])?;
    Ok(())
}

pub fn set_server_status(database_path: &Path, server_id: &str, status: &str) -> SqlResult<()> {
    let connection = open_connection(database_path)?;
    let connected_at: Option<String> = if status == "connected" {
        Some(now())
    } else {
        None
    };
    connection.execute(
        "UPDATE servers
         SET status = ?2,
             last_connected_at = COALESCE(?3, last_connected_at),
             updated_at = ?4
         WHERE id = ?1",
        params![server_id, status, connected_at, now()],
    )?;
    Ok(())
}

fn get_group(database_path: &Path, group_id: &str) -> SqlResult<ServerGroup> {
    let connection = open_connection(database_path)?;
    connection.query_row(
        "SELECT id, name, parent_id, sort_order FROM groups WHERE id = ?1",
        params![group_id],
        |row| {
            Ok(ServerGroup {
                id: row.get(0)?,
                name: row.get(1)?,
                parent_id: row.get(2)?,
                sort_order: row.get(3)?,
            })
        },
    )
}

pub fn get_server_secret_state(
    database_path: &Path,
    server_id: &str,
) -> SqlResult<ServerSecretState> {
    let connection = open_connection(database_path)?;
    get_server_secret_state_from_connection(&connection, server_id)
}

fn get_server_secret_state_from_connection(
    connection: &Connection,
    server_id: &str,
) -> SqlResult<ServerSecretState> {
    connection
        .query_row(
            "SELECT auth_type, credential_ref, private_key_ref, private_key_path FROM servers WHERE id = ?1",
            params![server_id],
            |row| {
                Ok(ServerSecretState {
                    auth_type: row.get(0)?,
                    credential_ref: row.get(1)?,
                    private_key_ref: row.get(2)?,
                    private_key_path: row.get(3)?,
                })
            },
        )
        .optional()
        .map(|value| {
            value.unwrap_or(ServerSecretState {
                auth_type: String::new(),
                credential_ref: None,
                private_key_ref: None,
                private_key_path: None,
            })
        })
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn open_connection(database_path: &Path) -> SqlResult<Connection> {
    let connection = Connection::open(database_path)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.busy_timeout(Duration::from_secs(5))?;
    Ok(connection)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("alax-{name}-{}.sqlite3", Uuid::new_v4()))
    }

    fn server_input(id: &str, host: &str) -> ServerInput {
        ServerInput {
            id: Some(id.to_string()),
            name: "Test".to_string(),
            host: host.to_string(),
            port: 22,
            username: "root".to_string(),
            auth_type: "password".to_string(),
            password: None,
            use_empty_password: false,
            private_key_path: None,
            private_key_content: None,
            passphrase: None,
            group_id: None,
            tags: vec![],
            note: String::new(),
        }
    }

    #[test]
    fn server_delete_cascades_host_keys_and_tunnels() {
        let path = database_path("cascade");
        initialize(&path).unwrap();
        let server_id = Uuid::new_v4().to_string();
        create_server(&path, server_input(&server_id, "127.0.0.1"), None, None).unwrap();
        trust_host_key(&path, &server_id, "ssh-ed25519", "SHA256:test").unwrap();
        save_tunnel_rule(
            &path,
            TunnelRuleInput {
                id: None,
                server_id: server_id.clone(),
                name: "Web".to_string(),
                local_host: "127.0.0.1".to_string(),
                local_port: 8080,
                remote_host: "127.0.0.1".to_string(),
                remote_port: 80,
            },
        )
        .unwrap();

        delete_server(&path, &server_id).unwrap();
        assert!(get_trusted_host_key(&path, &server_id).unwrap().is_none());
        assert!(list_tunnel_rules(&path, &server_id).unwrap().is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn endpoint_change_clears_trusted_host_key() {
        let path = database_path("endpoint");
        initialize(&path).unwrap();
        let server_id = Uuid::new_v4().to_string();
        create_server(&path, server_input(&server_id, "127.0.0.1"), None, None).unwrap();
        trust_host_key(&path, &server_id, "ssh-ed25519", "SHA256:test").unwrap();
        update_server(&path, server_input(&server_id, "127.0.0.2"), None, None).unwrap();
        assert!(get_trusted_host_key(&path, &server_id).unwrap().is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn endpoint_update_rolls_back_when_host_key_cleanup_fails() {
        let path = database_path("transaction");
        initialize(&path).unwrap();
        let server_id = Uuid::new_v4().to_string();
        create_server(&path, server_input(&server_id, "127.0.0.1"), None, None).unwrap();
        trust_host_key(&path, &server_id, "ssh-ed25519", "SHA256:test").unwrap();

        let connection = open_connection(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER reject_host_key_delete
                 BEFORE DELETE ON trusted_host_keys
                 BEGIN
                   SELECT RAISE(ABORT, 'test rollback');
                 END;",
            )
            .unwrap();

        assert!(update_server(&path, server_input(&server_id, "127.0.0.2"), None, None).is_err());
        assert_eq!(get_server(&path, &server_id).unwrap().host, "127.0.0.1");
        assert!(get_trusted_host_key(&path, &server_id).unwrap().is_some());
        drop(connection);
        let _ = std::fs::remove_file(path);
    }
}
