import { Copy, DatabaseZap, FolderPlus, Pencil, PlugZap, Server, Tags, TerminalSquare, Trash2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { GroupEditor } from "@/server/group-editor";
import { ServerEditor } from "@/server/server-editor";
import { duplicateServerProfile, isTauriRuntime, removeGroup, removeServer, testConnection } from "@/lib/tauri";
import { useAppStore } from "@/stores/app-store";

interface ContextMenuState {
  x: number;
  y: number;
  serverId: string;
  serverName: string;
}

export function ServerPanel() {
  const groups = useAppStore((state) => state.groups);
  const servers = useAppStore((state) => state.servers);
  const searchTerm = useAppStore((state) => state.searchTerm);
  const selectedGroupId = useAppStore((state) => state.selectedGroupId);
  const selectedServerId = useAppStore((state) => state.selectedServerId);
  const deleteServer = useAppStore((state) => state.deleteServer);
  const deleteGroup = useAppStore((state) => state.deleteGroup);
  const duplicateServer = useAppStore((state) => state.duplicateServer);
  const upsertServer = useAppStore((state) => state.upsertServer);
  const setSelectedGroupId = useAppStore((state) => state.setSelectedGroupId);
  const setSelectedServerId = useAppStore((state) => state.setSelectedServerId);
  const setServerStatus = useAppStore((state) => state.setServerStatus);
  const openTerminal = useAppStore((state) => state.openTerminal);
  const addLog = useAppStore((state) => state.addLog);
  const terminalTabs = useAppStore((state) => state.terminalTabs);
  const closeTerminal = useAppStore((state) => state.closeTerminal);
  const [serverEditorOpen, setServerEditorOpen] = useState(false);
  const [editingServerId, setEditingServerId] = useState<string | null>(null);
  const [groupEditorOpen, setGroupEditorOpen] = useState(false);
  const selectedServer = servers.find((server) => server.id === selectedServerId) ?? null;
  const editingServer = editingServerId ? servers.find((server) => server.id === editingServerId) ?? null : null;
  const selectedGroup = groups.find((group) => group.id === selectedGroupId) ?? null;
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const contextMenuRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    function handleClick(e: MouseEvent) {
      if (contextMenuRef.current && !contextMenuRef.current.contains(e.target as Node)) {
        setContextMenu(null);
      }
    }
    if (contextMenu) {
      document.addEventListener("mousedown", handleClick);
      return () => document.removeEventListener("mousedown", handleClick);
    }
  }, [contextMenu]);

  function handleContextMenu(e: React.MouseEvent, serverId: string, serverName: string) {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY, serverId, serverName });
  }

  const filteredServers = useMemo(() => {
    const term = searchTerm.trim().toLowerCase();
    return servers.filter((server) => {
      const inGroup = selectedGroupId ? server.groupId === selectedGroupId : true;
      const inSearch =
        !term ||
        [server.name, server.host, server.username, server.note, ...server.tags].some((value) => value.toLowerCase().includes(term));
      return inGroup && inSearch;
    });
  }, [searchTerm, selectedGroupId, servers]);

  async function runTest(serverId: string) {
    setServerStatus(serverId, "idle");
    try {
      const message = await testConnection(serverId);
      setServerStatus(serverId, "connected");
      addLog({ level: "info", category: "ssh", message });
    } catch (error) {
      setServerStatus(serverId, "error");
      addLog({ level: "error", category: "ssh", message: error instanceof Error ? error.message : String(error) });
    }
  }

  async function deleteSelectedGroup() {
    if (!selectedGroup) return;
    if (!confirm(`删除分组「${selectedGroup.name}」？服务器会移动到未分组。`)) return;
    try {
      await removeGroup(selectedGroup.id);
      deleteGroup(selectedGroup.id);
    } catch (error) {
      addLog({ level: "error", category: "group", message: error instanceof Error ? error.message : String(error) });
    }
  }

  async function deleteSelectedServer(serverId: string, serverName: string) {
    if (!confirm(`删除服务器「${serverName}」？此操作不会删除远程服务器上的任何文件。`)) return;
    try {
      await Promise.all(terminalTabs.filter((tab) => tab.serverId === serverId).map((tab) => closeTerminal(tab.id)));
      await removeServer(serverId);
      deleteServer(serverId);
    } catch (error) {
      addLog({ level: "error", category: "server", message: error instanceof Error ? error.message : String(error) });
    }
  }

  async function duplicateSelectedServer(serverId: string) {
    if (!isTauriRuntime()) {
      duplicateServer(serverId);
      return;
    }
    try {
      const saved = await duplicateServerProfile(serverId);
      if (saved) upsertServer({
        id: saved.id,
        name: saved.name,
        host: saved.host,
        port: saved.port,
        username: saved.username,
        authType: saved.authType,
        groupId: saved.groupId,
        tags: saved.tags,
        note: saved.note,
      }, saved);
    } catch (error) {
      addLog({ level: "error", category: "server", message: error instanceof Error ? error.message : String(error) });
    }
  }

  return (
    <aside className="flex w-72 shrink-0 flex-col border-r bg-card">
      <div className="flex h-14 items-center justify-between border-b px-3">
        <div>
          <div className="text-sm font-semibold">服务器</div>
          <div className="text-xs text-muted-foreground">分组、标签、连接状态</div>
        </div>
        <Button variant="ghost" size="icon" title="新增分组" onClick={() => setGroupEditorOpen(true)}>
          <FolderPlus size={17} />
        </Button>
      </div>
      <div className="border-b p-3">
        <div className="mb-2 flex items-center gap-2 text-xs font-medium text-muted-foreground">
          <DatabaseZap size={14} />
          分组
        </div>
        <div className="space-y-1">
          <GroupRow name="全部服务器" count={servers.length} active={!selectedGroupId} onClick={() => setSelectedGroupId(null)} />
          {groups.map((group) => (
            <GroupRow
              key={group.id}
              name={group.name}
              count={servers.filter((server) => server.groupId === group.id).length}
              active={selectedGroupId === group.id}
              onClick={() => setSelectedGroupId(group.id)}
            />
          ))}
          <GroupRow name="未分组" count={servers.filter((server) => !server.groupId).length} />
        </div>
        {selectedGroup ? (
          <div className="mt-2 flex gap-1">
            <Button variant="outline" size="sm" className="flex-1" onClick={() => setGroupEditorOpen(true)}>
              <Pencil size={13} />
              编辑
            </Button>
            <Button variant="outline" size="sm" className="flex-1" onClick={() => void deleteSelectedGroup()}>
              <Trash2 size={13} />
              删除
            </Button>
          </div>
        ) : null}
      </div>
      <div className="min-h-0 flex-1 overflow-auto p-3">
        <div className="mb-2 flex items-center gap-2 text-xs font-medium text-muted-foreground">
          <Server size={14} />
          服务器配置
        </div>
        <div className="space-y-2">
          {filteredServers.length === 0 ? (
            <div className="rounded-md border bg-background p-3 text-xs text-muted-foreground">还没有服务器配置，请点击下方新增服务器。</div>
          ) : null}
          {filteredServers.map((server) => (
            <div
              key={server.id}
              className={`rounded-md border p-3 transition-colors ${
                selectedServerId === server.id ? "border-primary bg-primary/8" : "bg-background hover:bg-muted"
              }`}
              onDoubleClick={() => void openTerminal(server.id)}
              onContextMenu={(e) => handleContextMenu(e, server.id, server.name)}
            >
              <button className="w-full text-left" onClick={() => setSelectedServerId(server.id)}>
                <div className="flex items-center justify-between gap-2">
                  <span className="truncate text-sm font-medium">{server.name}</span>
                  <Badge tone={server.status === "connected" || server.status === "available" ? "success" : "muted"}>
                    {server.status === "idle" ? "空闲" : server.status === "available" ? "可用" : server.status === "connected" ? "已连接" : "错误"}
                  </Badge>
                </div>
                <div className="mt-1 truncate text-xs text-muted-foreground">
                  {server.username}@{server.host}:{server.port}
                </div>
                <div className="mt-2 flex flex-wrap gap-1">
                  {server.tags.map((tag) => (
                    <span key={tag} className="inline-flex items-center gap-1 rounded bg-muted px-1.5 py-0.5 text-[11px] text-muted-foreground">
                      <Tags size={11} />
                      {tag}
                    </span>
                  ))}
                </div>
              </button>
              {selectedServerId === server.id ? (
                <div className="mt-3 grid grid-cols-4 gap-1">
                  <Button variant="ghost" size="icon" title="打开终端" onClick={() => void openTerminal(server.id)}>
                    <TerminalSquare size={14} />
                  </Button>
                  <Button variant="ghost" size="icon" title="测试连接" onClick={() => void runTest(server.id)}>
                    <PlugZap size={14} />
                  </Button>
                  <Button variant="ghost" size="icon" title="复制配置" onClick={() => void duplicateSelectedServer(server.id)}>
                    <Copy size={14} />
                  </Button>
                  <Button variant="ghost" size="icon" title="删除" onClick={() => void deleteSelectedServer(server.id, server.name)}>
                    <Trash2 size={14} />
                  </Button>
                </div>
              ) : null}
            </div>
          ))}
        </div>
      </div>
      <div className="grid grid-cols-2 gap-2 border-t p-3">
        <Button
          onClick={() => {
            setEditingServerId(null);
            setServerEditorOpen(true);
          }}
        >
          新增服务器
        </Button>
        <Button
          variant="outline"
          onClick={() => {
            if (!selectedServer) return;
            setEditingServerId(selectedServer.id);
            setServerEditorOpen(true);
          }}
          disabled={!selectedServer}
        >
          编辑
        </Button>
      </div>
      <ServerEditor
        open={serverEditorOpen}
        server={editingServer}
        onClose={() => {
          setServerEditorOpen(false);
          setEditingServerId(null);
        }}
      />
      <GroupEditor open={groupEditorOpen} group={selectedGroup} onClose={() => setGroupEditorOpen(false)} />
      {contextMenu ? (
        <div
          ref={contextMenuRef}
          className="fixed z-50 min-w-36 rounded-md border bg-popover p-1 shadow-md"
          style={{ left: contextMenu.x, top: contextMenu.y }}
        >
          <ContextMenuItem icon={<TerminalSquare size={14} />} label="打开终端" onClick={() => { openTerminal(contextMenu.serverId); setContextMenu(null); }} />
          <ContextMenuItem icon={<PlugZap size={14} />} label="测试连接" onClick={() => { runTest(contextMenu.serverId); setContextMenu(null); }} />
          <ContextMenuItem icon={<Copy size={14} />} label="复制配置" onClick={() => { void duplicateSelectedServer(contextMenu.serverId); setContextMenu(null); }} />
          <ContextMenuItem icon={<Pencil size={14} />} label="编辑" onClick={() => { setEditingServerId(contextMenu.serverId); setServerEditorOpen(true); setContextMenu(null); }} />
          <div className="my-1 border-t" />
          <ContextMenuItem icon={<Trash2 size={14} />} label="删除" className="text-red-500" onClick={() => { deleteSelectedServer(contextMenu.serverId, contextMenu.serverName); setContextMenu(null); }} />
        </div>
      ) : null}
    </aside>
  );
}

function ContextMenuItem({ icon, label, className = "", onClick }: { icon: React.ReactNode; label: string; className?: string; onClick: () => void }) {
  return (
    <button
      className={`flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-sm hover:bg-muted ${className}`}
      onClick={onClick}
    >
      {icon}
      {label}
    </button>
  );
}

function GroupRow({ name, count, active = false, onClick }: { name: string; count: number; active?: boolean; onClick?: () => void }) {
  return (
    <button onClick={onClick} className={`flex h-8 w-full items-center justify-between rounded-md px-2 text-sm ${active ? "bg-muted" : "hover:bg-muted"}`}>
      <span>{name}</span>
      <span className="text-xs text-muted-foreground">{count}</span>
    </button>
  );
}
