use chrono::Local;
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

const SENSITIVE_MARKERS: [&str; 5] = ["password", "passphrase", "private_key", "secret", "token"];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub category: String,
    pub message: String,
}

pub fn append_event(
    log_dir: &Path,
    level: &str,
    category: &str,
    message: &str,
) -> std::io::Result<()> {
    if let Err(error) = try_append_event(log_dir, level, category, message) {
        eprintln!("ALAX log write failed: {error}");
    }
    Ok(())
}

fn try_append_event(
    log_dir: &Path,
    level: &str,
    category: &str,
    message: &str,
) -> std::io::Result<()> {
    std::fs::create_dir_all(log_dir)?;
    let today = Local::now().format("%Y-%m-%d").to_string();
    let log_path = log_dir.join(format!("{today}.log"));
    let sanitized = sanitize(message);
    let line = format!(
        "{} [{}] [{}] {}\n",
        Local::now().format("%Y-%m-%d %H:%M:%S"),
        sanitize_label(level).to_uppercase(),
        sanitize_label(category),
        sanitized
    );

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    file.write_all(line.as_bytes())
}

pub fn sanitize(message: &str) -> String {
    let lower = message.to_lowercase();
    if SENSITIVE_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return "[已脱敏的敏感日志]".to_string();
    }

    let normalized: String = message
        .chars()
        .map(|character| {
            if matches!(character, '\r' | '\n' | '\t') {
                ' '
            } else if character.is_control() {
                '�'
            } else {
                character
            }
        })
        .collect();
    normalized.chars().take(4_096).collect()
}

pub fn read_recent_logs(log_dir: &Path, max_lines: usize) -> std::io::Result<Vec<LogEntry>> {
    if max_lines == 0 {
        return Ok(Vec::new());
    }
    let mut log_files: Vec<PathBuf> = Vec::new();
    if log_dir.exists() {
        for entry in std::fs::read_dir(log_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("log") {
                log_files.push(path);
            }
        }
    }
    log_files.sort_by(|a, b| b.cmp(a));

    let mut entries = Vec::new();
    for log_file in log_files {
        let content = std::fs::read_to_string(&log_file)?;
        for line in content.lines().rev() {
            if let Some(entry) = parse_log_line(line) {
                entries.push(entry);
                if entries.len() >= max_lines {
                    return Ok(entries);
                }
            }
        }
    }
    Ok(entries)
}

fn sanitize_label(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        .take(32)
        .collect()
}

fn parse_log_line(line: &str) -> Option<LogEntry> {
    let parts: Vec<&str> = line.splitn(4, " [").collect();
    if parts.len() != 4 && parts.len() != 3 {
        return None;
    }

    let timestamp = parts.first()?.to_string();
    let rest = line.trim_start_matches(&format!("{timestamp} ["));
    let parts: Vec<&str> = rest.split("] ").collect();
    if parts.len() < 3 {
        return None;
    }

    let level = parts[0].to_string();
    let category = parts[1].to_string();
    let message = parts[2..].join("] ");

    Some(LogEntry {
        timestamp,
        level,
        category,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::sanitize;

    #[test]
    fn removes_line_breaks_from_log_messages() {
        assert_eq!(sanitize("first\r\nsecond"), "first  second");
    }

    #[test]
    fn redacts_sensitive_messages() {
        assert_eq!(sanitize("password=example"), "[已脱敏的敏感日志]");
    }
}
