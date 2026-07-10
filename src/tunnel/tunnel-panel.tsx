import { Network, Pencil, Play, Plus, Square, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  deleteTunnelRule,
  listActiveTunnels,
  listTunnelRules,
  saveTunnelRule,
  startTunnel,
  stopTunnel,
  type TunnelRule,
  type TunnelRuleInput,
} from "@/lib/tauri";
import { useAppStore } from "@/stores/app-store";

const emptyRule = (serverId: string): TunnelRuleInput => ({
  serverId,
  name: "",
  localHost: "127.0.0.1",
  localPort: 8080,
  remoteHost: "127.0.0.1",
  remotePort: 80,
});

export function TunnelPanel() {
  const selectedServerId = useAppStore((state) => state.selectedServerId);
  const servers = useAppStore((state) => state.servers);
  const addLog = useAppStore((state) => state.addLog);
  const selectedServer = servers.find((server) => server.id === selectedServerId) ?? null;
  const [rules, setRules] = useState<TunnelRule[]>([]);
  const [activeIds, setActiveIds] = useState<Set<string>>(new Set());
  const [form, setForm] = useState<TunnelRuleInput>(() => emptyRule(selectedServerId ?? ""));
  const [busyId, setBusyId] = useState<string | null>(null);
  const editing = useMemo(() => rules.find((rule) => rule.id === form.id) ?? null, [form.id, rules]);

  useEffect(() => {
    if (!selectedServerId) {
      setRules([]);
      setActiveIds(new Set());
      setForm(emptyRule(""));
      return;
    }
    let cancelled = false;
    void Promise.all([listTunnelRules(selectedServerId), listActiveTunnels()])
      .then(([nextRules, active]) => {
        if (cancelled) return;
        setRules(nextRules);
        setActiveIds(new Set(active));
        setForm(emptyRule(selectedServerId));
      })
      .catch((error) => addLog({ level: "error", category: "tunnel", message: String(error) }));
    return () => {
      cancelled = true;
    };
  }, [addLog, selectedServerId]);

  useEffect(() => {
    if (!selectedServerId) return;
    let cancelled = false;
    let inFlight = false;
    const refresh = async () => {
      if (cancelled || inFlight || document.hidden) return;
      inFlight = true;
      try {
        const active = await listActiveTunnels();
        if (!cancelled) setActiveIds(new Set(active));
      } catch {
        // The next poll will reconcile transient backend or window lifecycle errors.
      } finally {
        inFlight = false;
      }
    };
    const timer = window.setInterval(() => void refresh(), 5_000);
    const onVisibilityChange = () => void refresh();
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [selectedServerId]);

  function update<K extends keyof TunnelRuleInput>(key: K, value: TunnelRuleInput[K]) {
    setForm((current) => ({ ...current, [key]: value }));
  }

  async function save() {
    if (!selectedServerId || !form.name.trim() || !form.remoteHost.trim()) return;
    setBusyId(form.id ?? "new");
    try {
      const saved = await saveTunnelRule({ ...form, serverId: selectedServerId });
      if (saved) {
        setRules((current) => [...current.filter((rule) => rule.id !== saved.id), saved].sort((a, b) => a.name.localeCompare(b.name, "zh-CN")));
        setForm(emptyRule(selectedServerId));
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      addLog({ level: "error", category: "tunnel", message });
      alert(`保存隧道失败：${message}`);
    } finally {
      setBusyId(null);
    }
  }

  async function toggle(rule: TunnelRule) {
    setBusyId(rule.id);
    try {
      if (activeIds.has(rule.id)) {
        await stopTunnel(rule.id);
        setActiveIds((current) => {
          const next = new Set(current);
          next.delete(rule.id);
          return next;
        });
      } else {
        await startTunnel(rule);
        setActiveIds((current) => new Set(current).add(rule.id));
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      addLog({ level: "error", category: "tunnel", message });
      alert(`隧道操作失败：${message}`);
    } finally {
      setBusyId(null);
    }
  }

  async function remove(rule: TunnelRule) {
    if (!confirm(`删除隧道「${rule.name}」？`)) return;
    setBusyId(rule.id);
    try {
      await deleteTunnelRule(rule.id);
      setRules((current) => current.filter((item) => item.id !== rule.id));
      setActiveIds((current) => {
        const next = new Set(current);
        next.delete(rule.id);
        return next;
      });
      if (form.id === rule.id && selectedServerId) setForm(emptyRule(selectedServerId));
    } catch (error) {
      alert(`删除隧道失败：${error instanceof Error ? error.message : String(error)}`);
    } finally {
      setBusyId(null);
    }
  }

  if (!selectedServer) {
    return <div className="flex h-full items-center justify-center text-sm text-muted-foreground">请选择服务器</div>;
  }

  return (
    <section className="h-full overflow-auto bg-background">
      <div className="border-b bg-card p-4">
        <div className="mb-3 flex items-center justify-between">
          <div className="flex items-center gap-2 font-medium">
            <Network size={17} />
            {editing ? "编辑本地隧道" : "新建本地隧道"}
          </div>
          {editing ? <Button variant="ghost" size="sm" onClick={() => setForm(emptyRule(selectedServer.id))}>取消编辑</Button> : null}
        </div>
        <div className="grid grid-cols-2 gap-2">
          <Input className="col-span-2" value={form.name} onChange={(event) => update("name", event.target.value)} placeholder="名称" />
          <Input value={form.localHost} disabled title="仅监听本机" />
          <Input type="number" min={1} max={65535} value={form.localPort} onChange={(event) => update("localPort", Number(event.target.value))} />
          <Input value={form.remoteHost} onChange={(event) => update("remoteHost", event.target.value)} placeholder="远程主机" />
          <Input type="number" min={1} max={65535} value={form.remotePort} onChange={(event) => update("remotePort", Number(event.target.value))} />
          <Button className="col-span-2 justify-self-end" onClick={() => void save()} disabled={busyId !== null || !form.name.trim() || !form.remoteHost.trim()}>
            <Plus size={15} />
            {editing ? "更新" : "保存"}
          </Button>
        </div>
      </div>
      <div className="p-4">
        <div className="mb-2 text-sm font-medium">{selectedServer.name}</div>
        <div className="space-y-2">
          {rules.length === 0 ? <div className="rounded-md border p-6 text-center text-sm text-muted-foreground">暂无隧道规则</div> : null}
          {rules.map((rule) => {
            const active = activeIds.has(rule.id);
            return (
              <div key={rule.id} className="grid grid-cols-[minmax(160px,1fr)_minmax(260px,2fr)_auto] items-center gap-3 rounded-md border bg-card px-4 py-3">
                <div className="min-w-0">
                  <div className="truncate text-sm font-medium">{rule.name}</div>
                  <div className={`text-xs ${active ? "text-emerald-500" : "text-muted-foreground"}`}>{active ? "运行中" : "已停止"}</div>
                </div>
                <div className="truncate font-mono text-xs text-muted-foreground">
                  {rule.localHost}:{rule.localPort} → {rule.remoteHost}:{rule.remotePort}
                </div>
                <div className="flex items-center gap-1">
                  <Button variant="outline" size="sm" onClick={() => void toggle(rule)} disabled={busyId === rule.id}>
                    {active ? <Square size={14} /> : <Play size={14} />}
                    {active ? "停止" : "启动"}
                  </Button>
                  <Button variant="ghost" size="icon" title="编辑" disabled={active} onClick={() => setForm({ ...rule })}>
                    <Pencil size={14} />
                  </Button>
                  <Button variant="ghost" size="icon" title="删除" onClick={() => void remove(rule)}>
                    <Trash2 size={14} />
                  </Button>
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </section>
  );
}
