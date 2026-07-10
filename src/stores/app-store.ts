import { create } from "zustand";
import { closeTerminalSession, isTauriRuntime, openTerminalSession } from "@/lib/tauri";
import { clearTerminalBuffer } from "@/terminal/terminal-registry";
import type {
  AppLogEntry,
  GroupFormInput,
  InitialSnapshot,
  RemoteFileEntry,
  RuntimeStatus,
  ServerFormInput,
  ServerGroup,
  ServerProfile,
  TerminalTab,
  ThemeMode,
  TransferTask,
  WorkspaceView,
} from "@/types/app";

const demoGroups: ServerGroup[] = [];
const demoServers: ServerProfile[] = [];
const demoFiles: RemoteFileEntry[] = [];

interface AppStore {
  runtimeStatus: RuntimeStatus;
  theme: ThemeMode;
  activeView: WorkspaceView;
  searchTerm: string;
  showHiddenFiles: boolean;
  selectedGroupId: string | null;
  selectedServerId: string | null;
  selectedFileIds: string[];
  activePath: string;
  groups: ServerGroup[];
  servers: ServerProfile[];
  files: RemoteFileEntry[];
  transfers: TransferTask[];
  terminalTabs: TerminalTab[];
  activeTerminalTabId: string | null;
  logs: AppLogEntry[];
  setTheme: (theme: ThemeMode) => void;
  setRuntimeStatus: (status: RuntimeStatus) => void;
  setActiveView: (view: WorkspaceView) => void;
  setSearchTerm: (value: string) => void;
  toggleHiddenFiles: () => void;
  setSelectedGroupId: (groupId: string | null) => void;
  setSelectedServerId: (serverId: string | null) => void;
  toggleFileSelection: (fileId: string) => void;
  clearFileSelection: () => void;
  setActivePath: (path: string) => void;
  setFiles: (files: RemoteFileEntry[]) => void;
  setSnapshot: (snapshot: InitialSnapshot) => void;
  upsertServer: (input: ServerFormInput, saved?: ServerProfile | null) => ServerProfile;
  deleteServer: (serverId: string) => void;
  duplicateServer: (serverId: string) => void;
  setServerStatus: (serverId: string, status: ServerProfile["status"]) => void;
  upsertGroup: (input: GroupFormInput, saved?: ServerGroup | null) => ServerGroup;
  deleteGroup: (groupId: string) => void;
  createRemoteEntry: (kind: RemoteFileEntry["kind"], name: string) => void;
  deleteSelectedFiles: () => void;
  renameSelectedFile: (name: string) => void;
  addTransferTask: (task: TransferTask) => void;
  startTransfer: (type: TransferTask["type"], fileName: string) => void;
  updateTransfer: (taskId: string, patch: Partial<TransferTask>) => void;
  cancelTransfer: (taskId: string) => void;
  retryTransfer: (taskId: string) => void;
  openTerminal: (serverId: string) => Promise<void>;
  addTerminalTab: (tab: TerminalTab) => void;
  setActiveTerminalTab: (tabId: string) => void;
  updateTerminalStatus: (tabId: string, status: TerminalTab["status"], message?: string | null) => void;
  closeTerminal: (tabId: string) => Promise<void>;
  reconnectTerminal: (tabId: string) => Promise<void>;
  addLog: (entry: Omit<AppLogEntry, "id" | "createdAt">) => void;
}

function nowText() {
  return new Date().toLocaleString("zh-CN", { hour12: false });
}

function makeId(prefix: string) {
  return `${prefix}-${crypto.randomUUID()}`;
}

function baseName(path: string) {
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

export const useAppStore = create<AppStore>((set, get) => ({
  runtimeStatus: "starting",
  theme: "dark",
  activeView: "files",
  searchTerm: "",
  showHiddenFiles: false,
  selectedGroupId: null,
  selectedServerId: demoServers[0]?.id ?? null,
  selectedFileIds: [],
  activePath: "/",
  groups: demoGroups,
  servers: demoServers,
  files: demoFiles,
  transfers: [],
  terminalTabs: [],
  activeTerminalTabId: null,
  logs: [{ id: "log-1", level: "info", category: "app", message: "应用启动，日志系统已就绪", createdAt: nowText() }],
  setTheme: (theme) => set({ theme }),
  setRuntimeStatus: (runtimeStatus) => set({ runtimeStatus }),
  setActiveView: (activeView) => set({ activeView }),
  setSearchTerm: (searchTerm) => set({ searchTerm }),
  toggleHiddenFiles: () => set((state) => ({ showHiddenFiles: !state.showHiddenFiles })),
  setSelectedGroupId: (selectedGroupId) => set({ selectedGroupId }),
  setSelectedServerId: (selectedServerId) => set({ selectedServerId }),
  toggleFileSelection: (fileId) =>
    set((state) => ({
      selectedFileIds: state.selectedFileIds.includes(fileId)
        ? state.selectedFileIds.filter((id) => id !== fileId)
        : [...state.selectedFileIds, fileId],
    })),
  clearFileSelection: () => set({ selectedFileIds: [] }),
  setActivePath: (activePath) => set({ activePath: activePath.trim() || "/" }),
  setFiles: (files) => set({ files, selectedFileIds: [] }),
  setSnapshot: (snapshot) =>
    set(() => ({
      groups: snapshot.groups,
      servers: snapshot.servers,
      selectedServerId: snapshot.servers[0]?.id ?? null,
    })),
  upsertServer: (input, saved) => {
    const server: ServerProfile =
      saved ?? {
        id: input.id ?? makeId("server"),
        name: input.name.trim(),
        host: input.host.trim(),
        port: input.port,
        username: input.username.trim(),
        authType: input.authType,
        groupId: input.groupId ?? null,
        tags: input.tags,
        note: input.note,
        status: "idle",
        lastConnectedAt: null,
      };

    set((state) => ({
      servers: state.servers.some((item) => item.id === server.id)
        ? state.servers.map((item) => (item.id === server.id ? server : item))
        : [server, ...state.servers],
      selectedServerId: server.id,
      logs: [{ id: makeId("log"), level: "info", category: "server", message: `保存服务器：${server.name}`, createdAt: nowText() }, ...state.logs],
    }));

    return server;
  },
  deleteServer: (serverId) =>
    set((state) => ({
      servers: state.servers.filter((server) => server.id !== serverId),
      selectedServerId: state.selectedServerId === serverId ? state.servers.find((server) => server.id !== serverId)?.id ?? null : state.selectedServerId,
      terminalTabs: state.terminalTabs.filter((tab) => tab.serverId !== serverId),
    })),
  duplicateServer: (serverId) =>
    set((state) => {
      const source = state.servers.find((server) => server.id === serverId);
      if (!source) return state;
      const copy = { ...source, id: makeId("server"), name: `${source.name} 副本`, status: "idle" as const, lastConnectedAt: null };
      return { servers: [copy, ...state.servers], selectedServerId: copy.id };
    }),
  setServerStatus: (serverId, status) =>
    set((state) => ({
      servers: state.servers.map((server) =>
        server.id === serverId ? { ...server, status, lastConnectedAt: status === "connected" ? nowText() : server.lastConnectedAt } : server,
      ),
    })),
  upsertGroup: (input, saved) => {
    const group: ServerGroup =
      saved ?? {
        id: input.id ?? makeId("group"),
        name: input.name.trim(),
        parentId: input.parentId ?? null,
        sortOrder: input.sortOrder,
      };

    set((state) => ({
      groups: state.groups.some((item) => item.id === group.id)
        ? state.groups.map((item) => (item.id === group.id ? group : item))
        : [...state.groups, group].sort((a, b) => a.sortOrder - b.sortOrder),
    }));

    return group;
  },
  deleteGroup: (groupId) =>
    set((state) => ({
      groups: state.groups.filter((group) => group.id !== groupId),
      servers: state.servers.map((server) => (server.groupId === groupId ? { ...server, groupId: null } : server)),
      selectedGroupId: state.selectedGroupId === groupId ? null : state.selectedGroupId,
    })),
  createRemoteEntry: (kind, name) =>
    set((state) => {
      const cleanName = name.trim();
      if (!cleanName) return state;
      const path = `${state.activePath.replace(/\/$/, "")}/${cleanName}`.replace("//", "/");
      return {
        files: [
          {
            id: makeId("file"),
            name: cleanName,
            path,
            kind,
            size: kind === "directory" ? 0 : 1,
            modifiedAt: nowText(),
            permissions: kind === "directory" ? "755" : "644",
            owner: "remote",
          },
          ...state.files,
        ],
      };
    }),
  deleteSelectedFiles: () =>
    set((state) => ({
      files: state.files.filter((file) => !state.selectedFileIds.includes(file.id)),
      selectedFileIds: [],
    })),
  renameSelectedFile: (name) =>
    set((state) => {
      const selectedId = state.selectedFileIds[0];
      if (!selectedId || !name.trim()) return state;
      return { files: state.files.map((file) => (file.id === selectedId ? { ...file, name: name.trim(), modifiedAt: nowText() } : file)) };
    }),
  addTransferTask: (task) =>
    set((state) => ({
      transfers: [task, ...state.transfers.filter((item) => item.id !== task.id)],
    })),
  startTransfer: (type, fileName) =>
    set((state) => {
      const server = state.servers.find((item) => item.id === state.selectedServerId);
      return {
        transfers: [
          {
            id: makeId("transfer"),
            serverName: server?.name ?? "未选择服务器",
            type,
            fileName,
            status: "running",
            progress: 0,
            speed: 0,
          },
          ...state.transfers,
        ],
      };
    }),
  updateTransfer: (taskId, patch) =>
    set((state) => ({
      transfers: state.transfers.map((task) => (task.id === taskId ? { ...task, ...patch } : task)),
    })),
  cancelTransfer: (taskId) =>
    set((state) => ({
      transfers: state.transfers.map((task) => (task.id === taskId ? { ...task, status: "cancelled", speed: 0, errorMessage: "已取消" } : task)),
    })),
  retryTransfer: (taskId) =>
    set((state) => ({
      transfers: state.transfers.map((task) => (task.id === taskId ? { ...task, status: "running", progress: 0, speed: 0, errorMessage: undefined } : task)),
    })),
  openTerminal: async (serverId) => {
    const server = get().servers.find((item) => item.id === serverId);
    if (!server) return;

    if (!isTauriRuntime()) {
      const id = makeId("term");
      get().addTerminalTab({
        id,
        serverId,
        serverName: server.name,
        status: "connected",
        output: `预览模式：${server.username}@${server.host}:${server.port}\r\n`,
        createdAt: nowText(),
      });
      return;
    }

    try {
      const session = await openTerminalSession(serverId, 100, 30);
      get().addTerminalTab({
        id: session.id,
        serverId,
        serverName: server.name,
        status: "connecting",
        output: `正在连接 ${server.username}@${server.host}:${server.port} ...\r\n`,
        createdAt: nowText(),
      });
    } catch (error) {
      get().addLog({ level: "error", category: "terminal", message: error instanceof Error ? error.message : String(error) });
    }
  },
  addTerminalTab: (tab) =>
    set((state) => ({
      terminalTabs: [...state.terminalTabs.filter((item) => item.id !== tab.id), tab],
      activeTerminalTabId: tab.id,
      activeView: "terminal",
    })),
  setActiveTerminalTab: (activeTerminalTabId) => set({ activeTerminalTabId }),
  updateTerminalStatus: (tabId, status) =>
    set((state) => {
      const currentTab = state.terminalTabs.find((tab) => tab.id === tabId);
      if (!currentTab) return state;
      const terminalTabs = state.terminalTabs.map((tab) => (tab.id === tabId ? { ...tab, status } : tab));
      const hasConnectedSibling = terminalTabs.some(
        (tab) => tab.serverId === currentTab.serverId && tab.status === "connected",
      );
      const nextServerStatus = status === "connected" || hasConnectedSibling
        ? "connected"
        : status === "error"
          ? "error"
          : "idle";
      return {
        terminalTabs,
        servers: state.servers.map((server) =>
          server.id === currentTab.serverId ? { ...server, status: nextServerStatus } : server,
        ),
      };
    }),
  closeTerminal: async (tabId) => {
    if (isTauriRuntime()) {
      await closeTerminalSession(tabId).catch((error) => get().addLog({ level: "warn", category: "terminal", message: String(error) }));
    }
    clearTerminalBuffer(tabId);
    set((state) => {
      const nextTabs = state.terminalTabs.filter((tab) => tab.id !== tabId);
      return {
        terminalTabs: nextTabs,
        activeTerminalTabId: state.activeTerminalTabId === tabId ? nextTabs.at(-1)?.id ?? null : state.activeTerminalTabId,
      };
    });
  },
  reconnectTerminal: async (tabId) => {
    const tab = get().terminalTabs.find((item) => item.id === tabId);
    if (!tab) return;
    await get().closeTerminal(tabId);
    await get().openTerminal(tab.serverId);
  },
  addLog: (entry) =>
    set((state) => ({
      logs: [{ ...entry, id: makeId("log"), createdAt: nowText() }, ...state.logs],
    })),
}));

export function mapBackendTransfer(task: {
  id: string;
  serverId: string;
  transferType: "upload" | "download";
  localPath: string;
  remotePath: string;
  status: TransferTask["status"];
  progress: number;
  speed: number;
  errorMessage?: string | null;
}): TransferTask {
  const server = useAppStore.getState().servers.find((item) => item.id === task.serverId);
  const path = task.transferType === "upload" ? task.localPath : task.remotePath;
  return {
    id: task.id,
    serverId: task.serverId,
    serverName: server?.name ?? task.serverId,
    type: task.transferType,
    fileName: baseName(path),
    status: task.status,
    progress: task.progress,
    speed: task.speed,
    localPath: task.localPath,
    remotePath: task.remotePath,
    errorMessage: task.errorMessage ?? undefined,
  };
}
