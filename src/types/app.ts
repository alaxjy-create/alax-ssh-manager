export type RuntimeStatus = "starting" | "ready" | "offline";

export type ThemeMode = "light" | "dark";

export type AuthType = "password" | "private_key" | "private_key_with_passphrase";

export type WorkspaceView = "files" | "terminal" | "tunnels" | "settings" | "about";

export interface ServerGroup {
  id: string;
  name: string;
  parentId?: string | null;
  sortOrder: number;
}

export interface ServerProfile {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  authType: AuthType;
  groupId?: string | null;
  tags: string[];
  note: string;
  status: "idle" | "available" | "connected" | "error";
  lastConnectedAt?: string | null;
}

export interface ServerFormInput {
  id?: string;
  name: string;
  host: string;
  port: number;
  username: string;
  authType: AuthType;
  password?: string;
  useEmptyPassword?: boolean;
  privateKeyPath?: string;
  privateKeyContent?: string;
  passphrase?: string;
  groupId?: string | null;
  tags: string[];
  note: string;
}

export interface GroupFormInput {
  id?: string;
  name: string;
  parentId?: string | null;
  sortOrder: number;
}

export interface RemoteFileEntry {
  id: string;
  name: string;
  path: string;
  kind: "directory" | "file" | "link";
  size: number;
  modifiedAt: string;
  permissions: string;
  owner: string;
  selected?: boolean;
}

export interface TransferTask {
  id: string;
  serverName: string;
  type: "upload" | "download";
  fileName: string;
  status: "queued" | "running" | "failed" | "done" | "cancelled";
  progress: number;
  speed: number;
  serverId?: string;
  localPath?: string;
  remotePath?: string;
  errorMessage?: string;
}

export interface TerminalTab {
  id: string;
  serverId: string;
  serverName: string;
  status: "connecting" | "connected" | "closed" | "error";
  output: string;
  createdAt: string;
}

export interface AppLogEntry {
  id: string;
  level: "info" | "warn" | "error";
  category: string;
  message: string;
  createdAt: string;
}

export interface InitialSnapshot {
  groups: ServerGroup[];
  servers: ServerProfile[];
}
