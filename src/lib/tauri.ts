import { invoke, isTauri } from "@tauri-apps/api/core";
import type { GroupFormInput, InitialSnapshot, RemoteFileEntry, ServerFormInput, ServerGroup, ServerProfile } from "@/types/app";

export interface TerminalSession {
  id: string;
  serverId: string;
  status: "connecting" | "connected" | "closed" | "error";
}

export interface BackendTransferTask {
  id: string;
  serverId: string;
  transferType: "upload" | "download";
  localPath: string;
  remotePath: string;
  status: "queued" | "running" | "failed" | "done" | "cancelled";
  progress: number;
  speed: number;
  errorMessage?: string | null;
}

export interface TransferInput {
  serverId: string;
  transferType: "upload" | "download";
  localPath: string;
  remotePath: string;
  entryKind?: "file" | "directory";
}

export interface RemotePreview {
  path: string;
  name: string;
  mime: string;
  previewKind: "image" | "video" | "audio" | "pdf" | "text" | "document" | "unknown";
  size: number;
  truncated: boolean;
  dataUrl?: string | null;
  text?: string | null;
  message?: string | null;
}

export interface RemoteTextFile {
  path: string;
  text: string;
  size: number;
  sha256: string;
}

export interface HostKeyStatus {
  status: "unknown" | "trusted" | "changed";
  algorithm: string;
  fingerprint: string;
  trustedFingerprint?: string | null;
}

export interface TunnelRule {
  id: string;
  serverId: string;
  name: string;
  localHost: string;
  localPort: number;
  remoteHost: string;
  remotePort: number;
}

export interface TunnelRuleInput extends Omit<TunnelRule, "id"> {
  id?: string;
}

const trustedForSession = new Set<string>();

const browserSnapshot: InitialSnapshot = {
  groups: [],
  servers: [],
};

export async function initializeDatabase() {
  if (!isTauriRuntime()) {
    return;
  }

  await invoke("initialize_database");
}

export async function loadInitialSnapshot(): Promise<InitialSnapshot> {
  if (!isTauriRuntime()) {
    return browserSnapshot;
  }

  const [groups, servers] = await Promise.all([
    invoke<InitialSnapshot["groups"]>("list_groups"),
    invoke<InitialSnapshot["servers"]>("list_servers"),
  ]);

  return { groups, servers };
}

export async function saveServer(input: ServerFormInput): Promise<ServerProfile | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  if (input.id) trustedForSession.delete(input.id);
  return input.id
    ? invoke<ServerProfile>("update_server", { input })
    : invoke<ServerProfile>("create_server", { input });
}

export async function removeServer(serverId: string): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await invoke("delete_server", { serverId });
  trustedForSession.delete(serverId);
}

export async function duplicateServerProfile(serverId: string): Promise<ServerProfile | null> {
  if (!isTauriRuntime()) return null;
  return invoke<ServerProfile>("duplicate_server", { serverId });
}

export async function getHostKeyStatus(serverId: string): Promise<HostKeyStatus | null> {
  if (!isTauriRuntime()) return null;
  return invoke<HostKeyStatus>("get_host_key_status", { serverId });
}

export async function ensureHostKeyTrusted(serverId: string, allowReplacement = false): Promise<void> {
  if (!isTauriRuntime() || (!allowReplacement && trustedForSession.has(serverId))) return;
  const status = await invoke<HostKeyStatus>("get_host_key_status", { serverId });
  if (status.status === "trusted") {
    trustedForSession.add(serverId);
    return;
  }

  if (status.status === "changed" && !allowReplacement) {
    throw new Error(
      `SSH 主机密钥与已保存记录不一致，连接已阻止。\n已保存：${status.trustedFingerprint ?? "未知"}\n当前：${status.fingerprint}\n请使用“测试连接”核实并重新信任。`,
    );
  }

  const warning = status.status === "changed"
    ? `警告：服务器主机密钥发生了变化。\n\n已保存：${status.trustedFingerprint ?? "未知"}\n当前：${status.algorithm} ${status.fingerprint}\n\n只有在确认服务器重装或密钥已合法更换后才应继续。是否替换信任记录？`
    : `首次连接需要确认服务器身份。\n\n算法：${status.algorithm}\n指纹：${status.fingerprint}\n\n请与服务器管理员或服务器上的 ssh-keygen 输出核对。是否信任？`;
  if (!window.confirm(warning)) {
    throw new Error("已取消 SSH 主机密钥信任，未建立连接。");
  }

  await invoke("trust_host_key", { serverId, fingerprint: status.fingerprint });
  trustedForSession.add(serverId);
}

export async function clearTrustedHostKey(serverId: string): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke("clear_trusted_host_key", { serverId });
  trustedForSession.delete(serverId);
}

export async function listTunnelRules(serverId: string): Promise<TunnelRule[]> {
  if (!isTauriRuntime()) return [];
  return invoke<TunnelRule[]>("list_tunnel_rules", { serverId });
}

export async function saveTunnelRule(input: TunnelRuleInput): Promise<TunnelRule | null> {
  if (!isTauriRuntime()) return null;
  return invoke<TunnelRule>("save_tunnel_rule", { input });
}

export async function startTunnel(rule: TunnelRule): Promise<TunnelRule | null> {
  if (!isTauriRuntime()) return null;
  await ensureHostKeyTrusted(rule.serverId);
  return invoke<TunnelRule>("start_tunnel", { tunnelId: rule.id });
}

export async function stopTunnel(tunnelId: string): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke("stop_tunnel", { tunnelId });
}

export async function listActiveTunnels(): Promise<string[]> {
  if (!isTauriRuntime()) return [];
  return invoke<string[]>("list_active_tunnels");
}

export async function deleteTunnelRule(tunnelId: string): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke("delete_tunnel_rule", { tunnelId });
}

export async function saveGroup(input: GroupFormInput): Promise<ServerGroup | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  return input.id
    ? invoke<ServerGroup>("update_group", { input })
    : invoke<ServerGroup>("create_group", { input });
}

export async function removeGroup(groupId: string): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await invoke("delete_group", { groupId });
}

export async function testConnection(serverId: string): Promise<string> {
  if (!isTauriRuntime()) {
    return "浏览器预览模式：已完成连接测试流程演示。";
  }

  await ensureHostKeyTrusted(serverId, true);
  return invoke<string>("test_connection", { serverId });
}

export async function readRemoteDirectory(serverId: string, path: string): Promise<RemoteFileEntry[]> {
  if (!isTauriRuntime()) {
    return [];
  }

  await ensureHostKeyTrusted(serverId);
  return invoke<RemoteFileEntry[]>("sftp_read_dir", { serverId, path });
}

export async function createRemoteDirectory(serverId: string, path: string): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await ensureHostKeyTrusted(serverId);
  await invoke("sftp_create_dir", { serverId, path });
}

export async function createRemoteFile(serverId: string, path: string): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await ensureHostKeyTrusted(serverId);
  await invoke("sftp_create_file", { serverId, path });
}

export async function deleteRemotePaths(serverId: string, paths: string[]): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await ensureHostKeyTrusted(serverId);
  await invoke("sftp_delete", { serverId, paths });
}

export async function renameRemotePath(serverId: string, from: string, to: string): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await ensureHostKeyTrusted(serverId);
  await invoke("sftp_rename", { serverId, from, to });
}

export async function uploadRemoteFile(serverId: string, localPath: string, remotePath: string): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await ensureHostKeyTrusted(serverId);
  await invoke("sftp_upload_file", { serverId, localPath, remotePath });
}

export async function downloadRemoteFile(serverId: string, remotePath: string, localPath: string): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await ensureHostKeyTrusted(serverId);
  await invoke("sftp_download_file", { serverId, remotePath, localPath });
}

export async function previewRemoteFile(serverId: string, remotePath: string): Promise<RemotePreview | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  await ensureHostKeyTrusted(serverId);
  return invoke<RemotePreview>("sftp_preview_file", { serverId, remotePath });
}

export async function compressRemotePaths(serverId: string, paths: string[], destination: string): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await ensureHostKeyTrusted(serverId);
  await invoke("sftp_compress_paths", { serverId, paths, destination });
}

export async function openRemoteFileWithSystem(serverId: string, remotePath: string): Promise<string | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  await ensureHostKeyTrusted(serverId);
  return invoke<string>("sftp_open_remote_file", { serverId, remotePath });
}

export async function readRemoteTextFile(serverId: string, remotePath: string): Promise<RemoteTextFile | null> {
  if (!isTauriRuntime()) return null;
  await ensureHostKeyTrusted(serverId);
  return invoke<RemoteTextFile>("sftp_read_text_file", { serverId, remotePath });
}

export async function writeRemoteTextFile(
  serverId: string,
  remotePath: string,
  text: string,
  expectedSha256: string,
): Promise<string | null> {
  if (!isTauriRuntime()) return null;
  await ensureHostKeyTrusted(serverId);
  return invoke<string>("sftp_write_text_file", { serverId, remotePath, text, expectedSha256 });
}

export async function setRemotePermissions(serverId: string, paths: string[], mode: string, recursive: boolean): Promise<void> {
  if (!isTauriRuntime()) return;
  await ensureHostKeyTrusted(serverId);
  await invoke("sftp_set_permissions", { serverId, paths, mode, recursive });
}

export async function calculateRemoteChecksum(
  serverId: string,
  remotePath: string,
  algorithm: "sha256" | "sha1" | "md5" = "sha256",
): Promise<string | null> {
  if (!isTauriRuntime()) return null;
  await ensureHostKeyTrusted(serverId);
  return invoke<string>("sftp_checksum", { serverId, remotePath, algorithm });
}

export async function openTerminalSession(serverId: string, cols = 100, rows = 30): Promise<TerminalSession> {
  if (!isTauriRuntime()) {
    return { id: crypto.randomUUID(), serverId, status: "connected" };
  }

  await ensureHostKeyTrusted(serverId);
  return invoke<TerminalSession>("open_terminal", { serverId, cols, rows });
}

export async function writeTerminalInput(sessionId: string, data: string): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await invoke("terminal_write", { sessionId, data });
}

export async function resizeTerminalSession(sessionId: string, cols: number, rows: number): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await invoke("terminal_resize", { sessionId, cols, rows });
}

export async function closeTerminalSession(sessionId: string): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await invoke("close_terminal", { sessionId });
}

export async function pickUploadFile(): Promise<string | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  const result = await invoke<{ path: string } | null>("pick_upload_file");
  return result?.path ?? null;
}

export async function pickUploadDirectory(): Promise<string | null> {
  if (!isTauriRuntime()) return null;
  const result = await invoke<{ path: string } | null>("pick_upload_directory");
  return result?.path ?? null;
}

export async function pickDownloadPath(defaultFileName: string): Promise<string | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  const result = await invoke<{ path: string } | null>("pick_download_path", { defaultFileName });
  return result?.path ?? null;
}

export async function pickDownloadDirectory(): Promise<string | null> {
  if (!isTauriRuntime()) return null;
  const result = await invoke<{ path: string } | null>("pick_download_directory");
  return result?.path ?? null;
}

export async function startTransferTask(input: TransferInput): Promise<BackendTransferTask> {
  if (!isTauriRuntime()) {
    return {
      id: crypto.randomUUID(),
      serverId: input.serverId,
      transferType: input.transferType,
      localPath: input.localPath,
      remotePath: input.remotePath,
      status: "running",
      progress: 0,
      speed: 0,
    };
  }

  await ensureHostKeyTrusted(input.serverId);
  return invoke<BackendTransferTask>("transfer_start", { input });
}

export async function cancelTransferTask(taskId: string): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await invoke("transfer_cancel", { taskId });
}

export async function retryTransferTask(taskId: string): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await invoke("transfer_retry", { taskId });
}

export async function getLogDirectory(): Promise<string | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<string>("get_log_directory");
}

export async function openLogDirectory(): Promise<string | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<string>("open_log_directory");
}

export interface AppInfo {
  version: string;
  credentialStore: string;
  logDirectory: string;
  databaseDirectory: string;
}

export async function getAppInfo(): Promise<AppInfo | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<AppInfo>("get_app_info");
}

export interface BackendLogEntry {
  timestamp: string;
  level: string;
  category: string;
  message: string;
}

export async function readBackendLogs(maxLines = 200): Promise<BackendLogEntry[]> {
  if (!isTauriRuntime()) {
    return [];
  }

  return invoke<BackendLogEntry[]>("read_logs", { maxLines });
}

export function isTauriRuntime() {
  return isTauri();
}

export async function getServerStats(serverId: string): Promise<{
  cpuUsage: number;
  memoryUsed: number;
  memoryTotal: number;
  swapUsed: number;
  swapTotal: number;
  disks: { mount: string; used: number; total: number }[];
  networks: { name: string; rxBytes: number; txBytes: number }[];
  temperature: number | null;
  uptime: number;
  loadAvg1: number;
  loadAvg5: number;
  loadAvg15: number;
}> {
  if (!isTauriRuntime()) {
    return {
      cpuUsage: 0,
      memoryUsed: 0,
      memoryTotal: 0,
      swapUsed: 0,
      swapTotal: 0,
      disks: [],
      networks: [],
      temperature: null,
      uptime: 0,
      loadAvg1: 0,
      loadAvg5: 0,
      loadAvg15: 0,
    };
  }

  await ensureHostKeyTrusted(serverId);
  return invoke("get_server_stats", { serverId });
}
