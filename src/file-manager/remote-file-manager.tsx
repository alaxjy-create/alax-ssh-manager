import {
  Archive,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  ChevronRight,
  Copy,
  Download,
  Eye,
  EyeOff,
  File,
  FilePlus,
  FileText,
  Folder,
  FolderPlus,
  FolderUp,
  Hash,
  Image as ImageIcon,
  Info,
  List,
  MoreHorizontal,
  Pencil,
  PlaySquare,
  RefreshCw,
  Save,
  Trash2,
  Upload,
} from "lucide-react";
import { forwardRef, useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  compressRemotePaths,
  calculateRemoteChecksum,
  createRemoteDirectory,
  createRemoteFile,
  deleteRemotePaths,
  isTauriRuntime,
  openRemoteFileWithSystem,
  pickDownloadPath,
  pickDownloadDirectory,
  pickUploadFile,
  pickUploadDirectory,
  previewRemoteFile,
  readRemoteTextFile,
  readRemoteDirectory,
  renameRemotePath,
  startTransferTask,
  setRemotePermissions,
  writeRemoteTextFile,
  type RemotePreview,
  type RemoteTextFile,
} from "@/lib/tauri";
import { formatBytes } from "@/lib/utils";
import { mapBackendTransfer, useAppStore } from "@/stores/app-store";
import type { RemoteFileEntry } from "@/types/app";

type ContextMenuState =
  | { type: "file"; x: number; y: number; entry: RemoteFileEntry }
  | { type: "blank"; x: number; y: number };

type SortKey = "name" | "modifiedAt" | "size" | "kind";
type SortOrder = "asc" | "desc";
type ViewMode = "details" | "compact";

export function RemoteFileManager() {
  const files = useAppStore((state) => state.files);
  const activePath = useAppStore((state) => state.activePath);
  const showHiddenFiles = useAppStore((state) => state.showHiddenFiles);
  const selectedServerId = useAppStore((state) => state.selectedServerId);
  const setActivePath = useAppStore((state) => state.setActivePath);
  const setFiles = useAppStore((state) => state.setFiles);
  const toggleHiddenFiles = useAppStore((state) => state.toggleHiddenFiles);
  const addTransferTask = useAppStore((state) => state.addTransferTask);
  const addLog = useAppStore((state) => state.addLog);

  const [isLoading, setIsLoading] = useState(false);
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const [sortKey, setSortKey] = useState<SortKey>("name");
  const [sortOrder, setSortOrder] = useState<SortOrder>("asc");
  const [viewMode, setViewMode] = useState<ViewMode>("details");
  const [addressValue, setAddressValue] = useState(activePath);
  const [pathHistory, setPathHistory] = useState<string[]>([activePath]);
  const [historyIndex, setHistoryIndex] = useState(0);
  const [propertyEntry, setPropertyEntry] = useState<RemoteFileEntry | null>(null);
  const [preview, setPreview] = useState<RemotePreview | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [editorFile, setEditorFile] = useState<RemoteTextFile | null>(null);

  const contextMenuRef = useRef<HTMLDivElement | null>(null);
  const pathParts = activePath.split("/").filter(Boolean);
  const selectedEntries = files.filter((file) => selectedPaths.has(file.path));
  const primarySelection = selectedEntries[0] ?? null;

  const visibleFiles = useMemo(() => {
    const filtered = showHiddenFiles ? files : files.filter((file) => !file.name.startsWith("."));
    return [...filtered].sort((a, b) => {
      const dirWeight = (entry: RemoteFileEntry) => (entry.kind === "directory" ? 0 : entry.kind === "link" ? 1 : 2);
      const direction = sortOrder === "asc" ? 1 : -1;
      if (sortKey === "kind" && dirWeight(a) !== dirWeight(b)) {
        return (dirWeight(a) - dirWeight(b)) * direction;
      }
      if (sortKey === "size" && a.size !== b.size) {
        return (a.size - b.size) * direction;
      }
      const left = String(a[sortKey] ?? "").toLowerCase();
      const right = String(b[sortKey] ?? "").toLowerCase();
      const compared = left.localeCompare(right, "zh-CN", { numeric: true });
      return compared === 0 ? a.name.localeCompare(b.name, "zh-CN", { numeric: true }) : compared * direction;
    });
  }, [files, showHiddenFiles, sortKey, sortOrder]);

  useEffect(() => {
    setAddressValue(activePath);
  }, [activePath]);

  useEffect(() => {
    if (!contextMenu) return;
    function close(e: MouseEvent) {
      if (contextMenuRef.current && !contextMenuRef.current.contains(e.target as Node)) {
        setContextMenu(null);
      }
    }
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [contextMenu]);

  useEffect(() => {
    if (isTauriRuntime() && selectedServerId) {
      void refreshDirectory();
    }
    setSelectedPaths(new Set());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedServerId, activePath]);

  useEffect(() => {
    if (!selectedServerId || selectedEntries.length !== 1 || primarySelection?.kind === "directory") {
      setPreview(null);
      setPreviewLoading(false);
      return;
    }

    let cancelled = false;
    setPreviewLoading(true);
    void previewRemoteFile(selectedServerId, primarySelection.path)
      .then((result) => {
        if (!cancelled) setPreview(result);
      })
      .catch((error) => {
        if (!cancelled) {
          setPreview(null);
          addLog({ level: "warn", category: "preview", message: error instanceof Error ? error.message : String(error) });
        }
      })
      .finally(() => {
        if (!cancelled) setPreviewLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [addLog, primarySelection?.kind, primarySelection?.path, selectedEntries.length, selectedServerId]);

  function joinRemotePath(base: string, name: string) {
    return `${base.replace(/\/$/, "")}/${name}`.replace("//", "/");
  }

  function parentPath(path: string) {
    const parts = path.split("/").filter(Boolean);
    if (parts.length <= 1) return "/";
    return `/${parts.slice(0, -1).join("/")}`;
  }

  function navigateTo(path: string, recordHistory = true) {
    const nextPath = path.trim() || "/";
    if (recordHistory) {
      setPathHistory((history) => {
        const trimmed = history.slice(0, historyIndex + 1);
        if (trimmed.at(-1) === nextPath) return trimmed;
        return [...trimmed, nextPath];
      });
      setHistoryIndex((index) => index + 1);
    }
    setActivePath(nextPath);
  }

  function goHistory(offset: -1 | 1) {
    const nextIndex = historyIndex + offset;
    const nextPath = pathHistory[nextIndex];
    if (!nextPath) return;
    setHistoryIndex(nextIndex);
    setActivePath(nextPath);
  }

  function togglePathSelection(path: string) {
    setSelectedPaths((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }

  function selectOnly(path: string) {
    setSelectedPaths(new Set([path]));
  }

  async function runSftpAction(action: () => Promise<void>, success: string) {
    if (!selectedServerId) {
      addLog({ level: "warn", category: "sftp", message: "请先选择服务器" });
      return;
    }
    try {
      setIsLoading(true);
      await action();
      addLog({ level: "info", category: "sftp", message: success });
      await refreshDirectory();
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      addLog({ level: "error", category: "sftp", message: msg });
      alert(`操作失败：${msg}`);
    } finally {
      setIsLoading(false);
      setContextMenu(null);
    }
  }

  async function refreshDirectory() {
    if (!selectedServerId || !isTauriRuntime()) return;
    try {
      setIsLoading(true);
      const entries = await readRemoteDirectory(selectedServerId, activePath);
      setFiles(entries);
      setSelectedPaths(new Set());
    } catch (error) {
      addLog({ level: "error", category: "sftp", message: error instanceof Error ? error.message : String(error) });
    } finally {
      setIsLoading(false);
    }
  }

  function createFolder() {
    const name = prompt("新建文件夹名称");
    const cleanName = validateEntryName(name);
    if (!cleanName || !selectedServerId || !isTauriRuntime()) return;
    void runSftpAction(() => createRemoteDirectory(selectedServerId, joinRemotePath(activePath, cleanName)), "远程文件夹已创建");
  }

  function createFile() {
    const name = prompt("新建文件名称");
    const cleanName = validateEntryName(name);
    if (!cleanName || !selectedServerId || !isTauriRuntime()) return;
    void runSftpAction(() => createRemoteFile(selectedServerId, joinRemotePath(activePath, cleanName)), "远程文件已创建");
  }

  function handleDelete(paths: string[]) {
    if (!paths.length) return;
    if (!confirm(`确认删除选中的 ${paths.length} 个项目？目录会递归删除。`)) return;
    if (!isTauriRuntime() || !selectedServerId) return;
    void runSftpAction(() => deleteRemotePaths(selectedServerId, [...paths]), "远程路径已删除");
  }

  function handleRename(entry: RemoteFileEntry) {
    const name = prompt("重命名为", entry.name);
    const cleanName = validateEntryName(name);
    if (!cleanName || !selectedServerId || !isTauriRuntime()) return;
    void runSftpAction(() => renameRemotePath(selectedServerId, entry.path, joinRemotePath(activePath, cleanName)), "远程路径已重命名");
  }

  async function handleCopyPath(path: string) {
    try {
      await navigator.clipboard.writeText(path);
      addLog({ level: "info", category: "sftp", message: `已复制路径：${path}` });
    } catch {
      addLog({ level: "warn", category: "sftp", message: "无法复制路径到剪贴板" });
    }
    setContextMenu(null);
  }

  async function uploadFile() {
    if (!selectedServerId) {
      addLog({ level: "warn", category: "transfer", message: "请先选择服务器" });
      return;
    }
    if (!isTauriRuntime()) return;

    const localPath = await pickUploadFile();
    const remoteName = localPath?.split(/[\\/]/).pop();
    if (!localPath || !remoteName) return;

    try {
      const task = await startTransferTask({
        serverId: selectedServerId,
        transferType: "upload",
        localPath,
        remotePath: joinRemotePath(activePath, remoteName),
        entryKind: "file",
      });
      addTransferTask(mapBackendTransfer(task));
    } catch (error) {
      addLog({ level: "error", category: "transfer", message: error instanceof Error ? error.message : String(error) });
    }
  }

  async function uploadDirectory() {
    if (!selectedServerId || !isTauriRuntime()) return;
    const localPath = await pickUploadDirectory();
    const remoteName = localPath?.split(/[\\/]/).filter(Boolean).pop();
    if (!localPath || !remoteName) return;
    try {
      const task = await startTransferTask({
        serverId: selectedServerId,
        transferType: "upload",
        localPath,
        remotePath: joinRemotePath(activePath, remoteName),
        entryKind: "directory",
      });
      addTransferTask(mapBackendTransfer(task));
    } catch (error) {
      addLog({ level: "error", category: "transfer", message: error instanceof Error ? error.message : String(error) });
    }
  }

  async function downloadFile(entry?: RemoteFileEntry) {
    const target = entry ?? selectedEntries[0];
    if (!target || !selectedServerId || !isTauriRuntime()) return;

    const localPath = target.kind === "directory" ? await pickDownloadDirectory() : await pickDownloadPath(target.name);
    if (!localPath) return;

    try {
      const task = await startTransferTask({
        serverId: selectedServerId,
        transferType: "download",
        localPath,
        remotePath: target.path,
        entryKind: target.kind === "directory" ? "directory" : "file",
      });
      addTransferTask(mapBackendTransfer(task));
    } catch (error) {
      addLog({ level: "error", category: "transfer", message: error instanceof Error ? error.message : String(error) });
    }
  }

  async function openWithSystem(entry: RemoteFileEntry) {
    if (!selectedServerId || !isTauriRuntime() || entry.kind === "directory") return;
    await runSftpAction(async () => {
      await openRemoteFileWithSystem(selectedServerId, entry.path);
    }, "已调用系统打开方式");
  }

  async function openTextEditor(entry: RemoteFileEntry) {
    if (!selectedServerId || !isTauriRuntime() || entry.kind !== "file") return;
    setContextMenu(null);
    setIsLoading(true);
    try {
      const file = await readRemoteTextFile(selectedServerId, entry.path);
      if (file) setEditorFile(file);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      addLog({ level: "error", category: "editor", message });
      alert(`无法打开文本编辑器：${message}`);
    } finally {
      setIsLoading(false);
    }
  }

  async function saveTextFile(text: string) {
    if (!selectedServerId || !editorFile) return;
    const nextHash = await writeRemoteTextFile(selectedServerId, editorFile.path, text, editorFile.sha256);
    if (!nextHash) return;
    setEditorFile({ ...editorFile, text, size: new TextEncoder().encode(text).length, sha256: nextHash });
    addLog({ level: "info", category: "editor", message: `已保存：${editorFile.path}` });
    await refreshDirectory();
  }

  async function applyPermissions(entry: RemoteFileEntry, mode: string, recursive: boolean) {
    if (!selectedServerId) return;
    const targets = selectedEntries.some((selected) => selected.path === entry.path) ? selectedEntries : [entry];
    await setRemotePermissions(selectedServerId, targets.map((target) => target.path), mode, recursive);
    addLog({ level: "info", category: "sftp", message: `已更新 ${targets.length} 个项目的权限` });
    setPropertyEntry(null);
    await refreshDirectory();
  }

  async function calculateChecksum(entry: RemoteFileEntry, algorithm: "sha256" | "sha1" | "md5") {
    if (!selectedServerId) return null;
    return calculateRemoteChecksum(selectedServerId, entry.path, algorithm);
  }

  function compressSelection() {
    if (!selectedServerId || !selectedEntries.length) return;
    const baseName = selectedEntries.length === 1 ? selectedEntries[0].name.replace(/[^\w.-]+/g, "_") : "selected-files";
    const destination = prompt("压缩包保存为", joinRemotePath(activePath, `${baseName}.tar.gz`));
    if (!destination) return;
    void runSftpAction(() => compressRemotePaths(selectedServerId, selectedEntries.map((entry) => entry.path), destination), "远程压缩包已创建");
  }

  function onRowContextMenu(e: React.MouseEvent, entry: RemoteFileEntry) {
    e.preventDefault();
    e.stopPropagation();
    if (!selectedPaths.has(entry.path)) {
      selectOnly(entry.path);
    }
    setContextMenu({ type: "file", x: e.clientX, y: e.clientY, entry });
  }

  function onBlankContextMenu(e: React.MouseEvent) {
    e.preventDefault();
    if ((e.target as HTMLElement).closest("[data-file-row='true']")) return;
    setContextMenu({ type: "blank", x: e.clientX, y: e.clientY });
  }

  function openEntry(entry: RemoteFileEntry) {
    if (entry.kind === "directory" || entry.kind === "link") {
      navigateTo(entry.path);
    } else if (isTextEntry(entry)) {
      void openTextEditor(entry);
    } else {
      void openWithSystem(entry);
    }
  }

  return (
    <section className="flex h-full min-h-0 flex-col bg-background">
      {contextMenu ? (
        <ContextMenu
          ref={contextMenuRef}
          state={contextMenu}
          selectedCount={selectedEntries.length}
          viewMode={viewMode}
          sortKey={sortKey}
          sortOrder={sortOrder}
          onSetView={setViewMode}
          onSetSort={(key) => {
            setSortOrder((order) => (sortKey === key ? (order === "asc" ? "desc" : "asc") : "asc"));
            setSortKey(key);
          }}
          onRefresh={() => void refreshDirectory()}
          onCreateFile={createFile}
          onCreateFolder={createFolder}
          onUpload={() => void uploadFile()}
          onUploadDirectory={() => void uploadDirectory()}
          onDownload={(entry) => void downloadFile(entry)}
          onOpen={openEntry}
          onEdit={(entry) => void openTextEditor(entry)}
          onOpenWith={(entry) => void openWithSystem(entry)}
          onCopy={(path) => void handleCopyPath(path)}
          onRename={handleRename}
          onCompress={compressSelection}
          onDelete={() => handleDelete([...selectedPaths])}
          onProperties={(entry) => {
            setPropertyEntry(entry ?? primarySelection ?? {
              id: `directory:${activePath}`,
              name: activePath === "/" ? "/" : activePath.split("/").filter(Boolean).at(-1) ?? activePath,
              path: activePath,
              kind: "directory",
              size: 0,
              modifiedAt: "-",
              permissions: "-",
              owner: `${files.length} 项`,
            });
            setContextMenu(null);
          }}
        />
      ) : null}

      <div className="flex h-12 shrink-0 items-center gap-2 border-b bg-card px-3">
        <Button variant="ghost" size="icon" title="后退" onClick={() => goHistory(-1)} disabled={historyIndex <= 0}>
          <ArrowLeft size={16} />
        </Button>
        <Button variant="ghost" size="icon" title="前进" onClick={() => goHistory(1)} disabled={historyIndex >= pathHistory.length - 1}>
          <ArrowRight size={16} />
        </Button>
        <Button variant="ghost" size="icon" title="上一层" onClick={() => navigateTo(parentPath(activePath))} disabled={activePath === "/"}>
          <ArrowUp size={16} />
        </Button>
        <Button variant="ghost" size="icon" title="刷新" onClick={() => void refreshDirectory()} disabled={isLoading}>
          <RefreshCw size={16} className={isLoading ? "animate-spin" : ""} />
        </Button>
        <div className="flex h-9 min-w-0 flex-1 items-center gap-1 rounded-md border bg-background px-3 text-sm">
          <button onClick={() => navigateTo("/")} className="text-primary">
            /
          </button>
          {pathParts.map((part, index) => (
            <span key={`${part}-${index}`} className="inline-flex min-w-0 items-center gap-1">
              <ChevronRight size={14} className="shrink-0 text-muted-foreground" />
              <button onClick={() => navigateTo(`/${pathParts.slice(0, index + 1).join("/")}`)} className="max-w-40 truncate hover:text-primary">
                {part}
              </button>
            </span>
          ))}
        </div>
        <Button variant="outline" size="sm" onClick={createFolder}>
          <FolderPlus size={15} />
          新建
        </Button>
        <Button variant="outline" size="sm" onClick={() => void uploadFile()}>
          <Upload size={15} />
          文件
        </Button>
        <Button variant="outline" size="sm" onClick={() => void uploadDirectory()}>
          <FolderUp size={15} />
          文件夹
        </Button>
      </div>

      <div className="flex h-11 shrink-0 items-center gap-2 border-b bg-card px-3">
        <form
          className="flex min-w-0 flex-1 gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            navigateTo(addressValue);
          }}
        >
          <Input value={addressValue} onChange={(event) => setAddressValue(event.target.value)} />
          <Button variant="outline" size="sm" type="submit">
            转到
          </Button>
        </form>
        <Button variant="outline" size="sm" onClick={() => setSelectedPaths(new Set())}>
          清除选择
        </Button>
        <span className="whitespace-nowrap text-xs text-muted-foreground">已选择 {selectedPaths.size} 项</span>
        <Button variant="ghost" size="icon" onClick={toggleHiddenFiles} title="显示隐藏文件">
          {showHiddenFiles ? <Eye size={15} /> : <EyeOff size={15} />}
        </Button>
      </div>

      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_300px]">
        <div className="min-h-0 overflow-auto p-3" onContextMenu={onBlankContextMenu}>
          <div className="overflow-hidden rounded-md border bg-card">
            <div className="grid grid-cols-[40px_minmax(220px,1fr)_110px_160px_120px_100px_44px] border-b bg-muted/60 px-3 py-2 text-xs font-medium text-muted-foreground">
              <div />
              <button className="text-left" onClick={() => setSortKey("name")}>名称</button>
              <button className="text-left" onClick={() => setSortKey("size")}>大小</button>
              <button className="text-left" onClick={() => setSortKey("modifiedAt")}>修改时间</button>
              <div>权限</div>
              <div>所有者</div>
              <div />
            </div>
            {visibleFiles.length === 0 ? (
              <div className="p-8 text-center text-sm text-muted-foreground">此文件夹为空</div>
            ) : null}
            {visibleFiles.map((entry) => (
              <div
                key={entry.path}
                data-file-row="true"
                onClick={() => selectOnly(entry.path)}
                onContextMenu={(e) => onRowContextMenu(e, entry)}
                onDoubleClick={() => openEntry(entry)}
                className={`grid grid-cols-[40px_minmax(220px,1fr)_110px_160px_120px_100px_44px] items-center border-b px-3 text-sm last:border-b-0 hover:bg-muted/60 ${
                  viewMode === "compact" ? "py-1.5" : "py-2"
                } ${selectedPaths.has(entry.path) ? "bg-primary/8" : ""}`}
              >
                <input
                  type="checkbox"
                  checked={selectedPaths.has(entry.path)}
                  onClick={(event) => event.stopPropagation()}
                  onChange={() => togglePathSelection(entry.path)}
                />
                <div className="flex min-w-0 items-center gap-2">
                  {entry.kind === "directory" ? <Folder size={17} className="text-amber-500" /> : fileIcon(entry)}
                  <span className="truncate">{entry.name}</span>
                </div>
                <div className="text-muted-foreground">{entry.kind === "directory" ? "-" : formatBytes(entry.size)}</div>
                <div className="text-muted-foreground">{entry.modifiedAt}</div>
                <div className="font-mono text-xs text-muted-foreground">{entry.permissions}</div>
                <div className="text-muted-foreground">{entry.owner}</div>
                <Button variant="ghost" size="icon" title="复制路径" onClick={(event) => { event.stopPropagation(); void handleCopyPath(entry.path); }}>
                  <Copy size={14} />
                </Button>
              </div>
            ))}
          </div>
        </div>
        <PreviewPanel entry={primarySelection} preview={preview} loading={previewLoading} />
      </div>

      {propertyEntry ? (
        <PropertiesDialog
          entry={propertyEntry}
          selectedCount={selectedEntries.some((entry) => entry.path === propertyEntry.path) ? selectedEntries.length : 1}
          onApplyPermissions={applyPermissions}
          onCalculateChecksum={calculateChecksum}
          onClose={() => setPropertyEntry(null)}
        />
      ) : null}
      {editorFile ? (
        <TextEditorDialog
          file={editorFile}
          onSave={saveTextFile}
          onClose={() => setEditorFile(null)}
        />
      ) : null}
    </section>
  );
}

interface ContextMenuProps {
  state: ContextMenuState;
  selectedCount: number;
  viewMode: ViewMode;
  sortKey: SortKey;
  sortOrder: SortOrder;
  onSetView: (mode: ViewMode) => void;
  onSetSort: (key: SortKey) => void;
  onRefresh: () => void;
  onCreateFile: () => void;
  onCreateFolder: () => void;
  onUpload: () => void;
  onUploadDirectory: () => void;
  onDownload: (entry: RemoteFileEntry) => void;
  onOpen: (entry: RemoteFileEntry) => void;
  onEdit: (entry: RemoteFileEntry) => void;
  onOpenWith: (entry: RemoteFileEntry) => void;
  onCopy: (path: string) => void;
  onRename: (entry: RemoteFileEntry) => void;
  onCompress: () => void;
  onDelete: () => void;
  onProperties: (entry?: RemoteFileEntry | null) => void;
}

const ContextMenu = forwardRef<HTMLDivElement, ContextMenuProps>(function ContextMenu(
  {
    state,
    selectedCount,
    viewMode,
    sortKey,
    sortOrder,
    onSetView,
    onSetSort,
    onRefresh,
    onCreateFile,
    onCreateFolder,
    onUpload,
    onUploadDirectory,
    onDownload,
    onOpen,
    onEdit,
    onOpenWith,
    onCopy,
    onRename,
    onCompress,
    onDelete,
    onProperties,
  },
  ref,
) {
  const entry = state.type === "file" ? state.entry : null;
  return (
    <div ref={ref} className="fixed z-50 min-w-[220px] rounded-md border bg-popover p-1 shadow-lg" style={{ left: state.x, top: state.y }}>
      {entry ? (
        <>
          <MenuLabel>{entry.name}</MenuLabel>
          <MenuItem icon={<Folder size={14} />} label="打开" onClick={() => onOpen(entry)} />
          {entry.kind === "file" && isTextEntry(entry) ? <MenuItem icon={<Pencil size={14} />} label="编辑文本" onClick={() => onEdit(entry)} /> : null}
          {entry.kind !== "directory" ? <MenuItem icon={<MoreHorizontal size={14} />} label="打开方式" onClick={() => onOpenWith(entry)} /> : null}
          <MenuItem icon={<Download size={14} />} label={entry.kind === "directory" ? "下载文件夹" : "下载"} onClick={() => onDownload(entry)} />
          <MenuItem icon={<Copy size={14} />} label="复制路径" onClick={() => onCopy(entry.path)} />
          <MenuItem icon={<Pencil size={14} />} label="重命名" onClick={() => onRename(entry)} />
          <MenuSeparator />
          <MenuItem icon={<Archive size={14} />} label={selectedCount > 1 ? `压缩 ${selectedCount} 项` : "压缩"} onClick={onCompress} />
          <MenuItem icon={<Info size={14} />} label="属性" onClick={() => onProperties(entry)} />
          <MenuSeparator />
          <MenuItem danger icon={<Trash2 size={14} />} label="删除" onClick={onDelete} />
        </>
      ) : (
        <>
          <MenuLabel>文件夹</MenuLabel>
          <MenuItem icon={<List size={14} />} label={`查看：${viewMode === "details" ? "详细信息" : "紧凑列表"}`} onClick={() => onSetView(viewMode === "details" ? "compact" : "details")} />
          <MenuItem icon={<RefreshCw size={14} />} label={`排序：${sortLabel(sortKey)} ${sortOrder === "asc" ? "↑" : "↓"}`} onClick={() => onSetSort(nextSortKey(sortKey))} />
          <MenuItem icon={<Eye size={14} />} label="刷新" onClick={onRefresh} />
          <MenuSeparator />
          <MenuItem icon={<FolderPlus size={14} />} label="新建文件夹" onClick={onCreateFolder} />
          <MenuItem icon={<FilePlus size={14} />} label="新建文件" onClick={onCreateFile} />
          <MenuItem icon={<Upload size={14} />} label="上传" onClick={onUpload} />
          <MenuItem icon={<FolderUp size={14} />} label="上传文件夹" onClick={onUploadDirectory} />
          <MenuSeparator />
          <MenuItem icon={<Info size={14} />} label="属性" onClick={() => onProperties(null)} />
        </>
      )}
    </div>
  );
});

function MenuLabel({ children }: { children: React.ReactNode }) {
  return <div className="max-w-64 truncate px-2 py-1 text-xs text-muted-foreground">{children}</div>;
}

function MenuSeparator() {
  return <div className="my-1 border-t" />;
}

function MenuItem({ icon, label, danger, onClick }: { icon: React.ReactNode; label: string; danger?: boolean; onClick: () => void }) {
  return (
    <button className={`flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs hover:bg-muted ${danger ? "text-red-500" : ""}`} onClick={onClick}>
      {icon}
      <span className="truncate">{label}</span>
    </button>
  );
}

function PreviewPanel({ entry, preview, loading }: { entry: RemoteFileEntry | null; preview: RemotePreview | null; loading: boolean }) {
  return (
    <aside className="min-h-0 border-l bg-card p-3">
      <div className="mb-3 flex items-center gap-2 text-sm font-medium">
        <Info size={16} />
        预览
      </div>
      {!entry ? <div className="text-sm text-muted-foreground">选择一个文件查看预览和属性。</div> : null}
      {entry ? (
        <div className="space-y-3">
          <div className="flex items-center gap-2">
            {entry.kind === "directory" ? <Folder className="text-amber-500" size={18} /> : fileIcon(entry)}
            <div className="min-w-0">
              <div className="truncate text-sm font-medium">{entry.name}</div>
              <div className="text-xs text-muted-foreground">{entry.kind === "directory" ? "文件夹" : formatBytes(entry.size)}</div>
            </div>
          </div>
          {entry.kind === "directory" ? <Metadata entry={entry} /> : null}
          {entry.kind !== "directory" && loading ? <div className="rounded-md border bg-background p-3 text-sm text-muted-foreground">正在生成预览...</div> : null}
          {entry.kind !== "directory" && !loading && preview ? <PreviewContent preview={preview} /> : null}
          {entry.kind !== "directory" ? <Metadata entry={entry} /> : null}
        </div>
      ) : null}
    </aside>
  );
}

function PreviewContent({ preview }: { preview: RemotePreview }) {
  if (preview.message) {
    return <div className="rounded-md border bg-background p-3 text-sm text-muted-foreground">{preview.message}</div>;
  }
  if (preview.previewKind === "image" && preview.dataUrl) {
    return <img src={preview.dataUrl} alt={preview.name} className="max-h-72 w-full rounded-md border object-contain" />;
  }
  if (preview.previewKind === "video" && preview.dataUrl) {
    return <video src={preview.dataUrl} controls className="max-h-72 w-full rounded-md border" />;
  }
  if (preview.previewKind === "audio" && preview.dataUrl) {
    return <audio src={preview.dataUrl} controls className="w-full" />;
  }
  if (preview.previewKind === "pdf" && preview.dataUrl) {
    return <iframe sandbox="" src={preview.dataUrl} title={preview.name} className="h-72 w-full rounded-md border bg-background" />;
  }
  if (preview.previewKind === "text" && preview.text) {
    return <pre className="max-h-72 overflow-auto rounded-md border bg-background p-3 text-xs leading-relaxed">{preview.text}</pre>;
  }
  return <div className="rounded-md border bg-background p-3 text-sm text-muted-foreground">暂无可用预览。</div>;
}

function Metadata({ entry }: { entry: RemoteFileEntry }) {
  return (
    <dl className="grid grid-cols-[64px_minmax(0,1fr)] gap-x-2 gap-y-1 text-xs text-muted-foreground">
      <dt>路径</dt>
      <dd className="break-all">{entry.path}</dd>
      <dt>权限</dt>
      <dd>{entry.permissions}</dd>
      <dt>所有者</dt>
      <dd>{entry.owner}</dd>
      <dt>修改</dt>
      <dd>{entry.modifiedAt}</dd>
    </dl>
  );
}

function PropertiesDialog({
  entry,
  selectedCount,
  onApplyPermissions,
  onCalculateChecksum,
  onClose,
}: {
  entry: RemoteFileEntry;
  selectedCount: number;
  onApplyPermissions: (entry: RemoteFileEntry, mode: string, recursive: boolean) => Promise<void>;
  onCalculateChecksum: (entry: RemoteFileEntry, algorithm: "sha256" | "sha1" | "md5") => Promise<string | null>;
  onClose: () => void;
}) {
  const [mode, setMode] = useState(entry.permissions === "-" ? "" : entry.permissions);
  const [recursive, setRecursive] = useState(false);
  const [algorithm, setAlgorithm] = useState<"sha256" | "sha1" | "md5">("sha256");
  const [checksum, setChecksum] = useState<string | null>(null);
  const [working, setWorking] = useState(false);

  async function applyPermissions() {
    setWorking(true);
    try {
      await onApplyPermissions(entry, mode, recursive);
    } catch (error) {
      alert(`修改权限失败：${error instanceof Error ? error.message : String(error)}`);
    } finally {
      setWorking(false);
    }
  }

  async function calculate() {
    setWorking(true);
    try {
      setChecksum(await onCalculateChecksum(entry, algorithm));
    } catch (error) {
      alert(`计算校验和失败：${error instanceof Error ? error.message : String(error)}`);
    } finally {
      setWorking(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
      <div className="w-[520px] rounded-md border bg-card shadow-xl">
        <div className="flex items-center justify-between border-b px-4 py-3">
          <div className="font-medium">属性</div>
          <Button variant="ghost" size="sm" onClick={onClose}>关闭</Button>
        </div>
        <div className="space-y-4 p-4">
          <div className="flex items-center gap-3">
            {entry.kind === "directory" ? <Folder className="text-amber-500" size={28} /> : fileIcon(entry)}
            <div className="min-w-0">
              <div className="truncate font-medium">{entry.name}</div>
              <div className="text-sm text-muted-foreground">
                {selectedCount > 1 ? `${selectedCount} 个项目` : entry.kind === "directory" ? "文件夹" : formatBytes(entry.size)}
              </div>
            </div>
          </div>
          <Metadata entry={entry} />
          <div className="border-t pt-4">
            <div className="mb-2 text-sm font-medium">Unix 权限</div>
            <div className="flex items-center gap-2">
              <Input
                value={mode}
                onChange={(event) => setMode(event.target.value.replace(/[^0-7]/g, "").slice(0, 4))}
                placeholder="例如 644 或 0755"
                disabled={entry.permissions === "-"}
              />
              <Button
                variant="outline"
                onClick={() => void applyPermissions()}
                disabled={working || entry.permissions === "-" || !/^[0-7]{3,4}$/.test(mode)}
              >
                应用
              </Button>
            </div>
            {entry.kind === "directory" && entry.permissions !== "-" ? (
              <label className="mt-2 flex items-center gap-2 text-xs text-muted-foreground">
                <input type="checkbox" checked={recursive} onChange={(event) => setRecursive(event.target.checked)} />
                递归应用到文件夹内容
              </label>
            ) : null}
          </div>
          {entry.kind === "file" ? (
            <div className="border-t pt-4">
              <div className="mb-2 flex items-center gap-2 text-sm font-medium">
                <Hash size={15} />
                校验和
              </div>
              <div className="flex gap-2">
                <select
                  className="h-9 rounded-md border bg-background px-3 text-sm"
                  value={algorithm}
                  onChange={(event) => {
                    setAlgorithm(event.target.value as "sha256" | "sha1" | "md5");
                    setChecksum(null);
                  }}
                >
                  <option value="sha256">SHA-256</option>
                  <option value="sha1">SHA-1</option>
                  <option value="md5">MD5</option>
                </select>
                <Button variant="outline" onClick={() => void calculate()} disabled={working}>计算</Button>
                <Button
                  variant="ghost"
                  size="icon"
                  title="复制校验和"
                  disabled={!checksum}
                  onClick={() => checksum && void navigator.clipboard.writeText(checksum)}
                >
                  <Copy size={14} />
                </Button>
              </div>
              {checksum ? <div className="mt-2 break-all rounded-md border bg-background p-2 font-mono text-xs">{checksum}</div> : null}
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}

function TextEditorDialog({ file, onSave, onClose }: { file: RemoteTextFile; onSave: (text: string) => Promise<void>; onClose: () => void }) {
  const [draft, setDraft] = useState(file.text);
  const [saving, setSaving] = useState(false);
  const dirty = draft !== file.text;

  useEffect(() => {
    setDraft(file.text);
  }, [file.path, file.sha256, file.text]);

  async function save() {
    if (!dirty || saving) return;
    setSaving(true);
    try {
      await onSave(draft);
    } catch (error) {
      alert(`保存失败：${error instanceof Error ? error.message : String(error)}`);
    } finally {
      setSaving(false);
    }
  }

  function close() {
    if (!dirty || confirm("文件尚未保存，确认关闭编辑器？")) onClose();
  }

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/50 p-6">
      <div className="flex h-[min(760px,90vh)] w-[min(1000px,94vw)] flex-col overflow-hidden rounded-md border bg-card shadow-xl">
        <div className="flex h-12 shrink-0 items-center justify-between gap-3 border-b px-4">
          <div className="min-w-0">
            <div className="truncate text-sm font-medium">{file.path}</div>
            <div className="text-xs text-muted-foreground">UTF-8 · {formatBytes(new TextEncoder().encode(draft).length)}{dirty ? " · 已修改" : ""}</div>
          </div>
          <div className="flex items-center gap-2">
            <Button variant="outline" onClick={close}>关闭</Button>
            <Button onClick={() => void save()} disabled={!dirty || saving}>
              <Save size={15} />
              {saving ? "保存中" : "保存"}
            </Button>
          </div>
        </div>
        <textarea
          className="min-h-0 flex-1 resize-none bg-background p-4 font-mono text-sm leading-6 outline-none"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          spellCheck={false}
        />
      </div>
    </div>
  );
}

function fileIcon(entry: RemoteFileEntry) {
  const ext = entry.name.split(".").pop()?.toLowerCase();
  if (["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"].includes(ext ?? "")) return <ImageIcon size={17} className="text-sky-400" />;
  if (["mp4", "webm", "mov", "mp3", "wav", "ogg"].includes(ext ?? "")) return <PlaySquare size={17} className="text-rose-400" />;
  if (["txt", "log", "md", "json", "pdf", "doc", "docx"].includes(ext ?? "")) return <FileText size={17} className="text-primary" />;
  return <File size={17} className="text-primary" />;
}

function isTextEntry(entry: RemoteFileEntry) {
  const ext = entry.name.split(".").pop()?.toLowerCase() ?? "";
  return ["txt", "log", "md", "json", "toml", "yaml", "yml", "xml", "csv", "ini", "conf", "sh", "bash", "zsh", "fish", "rs", "py", "go", "java", "c", "h", "cpp", "hpp", "ts", "tsx", "js", "jsx", "css", "html", "htm", "env", "service"].includes(ext)
    || !entry.name.includes(".");
}

function validateEntryName(value: string | null) {
  const name = value?.trim() ?? "";
  if (!name) return null;
  if (name === "." || name === ".." || /[\\/\u0000-\u001f\u007f]/.test(name)) {
    alert("名称不能是 . 或 ..，也不能包含斜杠或控制字符。");
    return null;
  }
  return name;
}

function sortLabel(key: SortKey) {
  return ({ name: "名称", modifiedAt: "修改时间", size: "大小", kind: "类型" } satisfies Record<SortKey, string>)[key];
}

function nextSortKey(key: SortKey): SortKey {
  return key === "name" ? "modifiedAt" : key === "modifiedAt" ? "size" : key === "size" ? "kind" : "name";
}
