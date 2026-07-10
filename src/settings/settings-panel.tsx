import { FolderOpen, Info, LockKeyhole, RefreshCw, Settings } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { type AppInfo, type BackendLogEntry, getAppInfo, openLogDirectory, readBackendLogs } from "@/lib/tauri";
import { useAppStore } from "@/stores/app-store";

export function SettingsPanel({ full = false }: { full?: boolean }) {
  const logs = useAppStore((state) => state.logs);
  const addLog = useAppStore((state) => state.addLog);
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null);
  const [backendLogs, setBackendLogs] = useState<BackendLogEntry[]>([]);
  const [logMode, setLogMode] = useState<"client" | "backend">("client");

  useEffect(() => {
    void getAppInfo().then((info) => setAppInfo(info));
  }, []);

  async function openLogs() {
    try {
      const dir = await openLogDirectory();
      addLog({ level: "info", category: "logs", message: dir ? `已打开日志目录：${dir}` : "浏览器预览模式暂不能打开本地日志目录" });
    } catch (error) {
      addLog({ level: "error", category: "logs", message: error instanceof Error ? error.message : String(error) });
    }
  }

  async function loadBackendLogs() {
    try {
      const entries = await readBackendLogs(200);
      setBackendLogs(entries);
    } catch (error) {
      addLog({ level: "error", category: "logs", message: error instanceof Error ? error.message : String(error) });
    }
  }

  return (
    <section className={`${full ? "h-full bg-background p-6" : "min-h-0 p-3"} overflow-auto`}>
      <div className="mb-3 flex items-center gap-2">
        <Settings size={16} />
        <div className="text-sm font-medium">设置</div>
      </div>
      <div className="space-y-2">
        <SettingRow icon={<LockKeyhole size={15} />} title="安全凭据" description={appInfo ? `使用 ${appInfo.credentialStore} 系统凭据存储` : "密码和 passphrase 只保存到系统凭据存储"} />
        <SettingRow icon={<FolderOpen size={15} />} title="日志目录" description={appInfo?.logDirectory ?? "按日期写入本地日志，默认脱敏"} />
        <SettingRow icon={<Info size={15} />} title="关于" description={appInfo ? `ALAX SSH Manager v${appInfo.version}` : "ALAX SSH Manager"} />
      </div>
      <Button className="mt-3 w-full" variant="outline" onClick={() => void openLogs()}>
        打开日志目录
      </Button>
      {full ? (
        <div className="mt-5">
          <div className="mb-2 flex items-center justify-between">
            <div className="text-sm font-medium">最近日志</div>
            <div className="flex items-center gap-2">
              <Button
                variant="outline"
                size="sm"
                onClick={() => {
                  setLogMode("client");
                }}
                disabled={logMode === "client"}
              >
                客户端
              </Button>
              <Button
                variant="outline"
                size="sm"
                onClick={() => {
                  setLogMode("backend");
                  void loadBackendLogs();
                }}
                disabled={logMode === "backend"}
              >
                后端
              </Button>
              {logMode === "backend" ? (
                <Button variant="ghost" size="icon" title="刷新后端日志" onClick={() => void loadBackendLogs()}>
                  <RefreshCw size={14} />
                </Button>
              ) : null}
            </div>
          </div>
          <div className="overflow-hidden rounded-md border bg-card">
            {logMode === "client"
              ? logs.map((log) => (
                  <div key={log.id} className="grid grid-cols-[150px_80px_100px_minmax(0,1fr)] gap-2 border-b px-3 py-2 text-xs last:border-b-0">
                    <span className="text-muted-foreground">{log.createdAt}</span>
                    <span>{log.level}</span>
                    <span>{log.category}</span>
                    <span className="truncate">{log.message}</span>
                  </div>
                ))
              : backendLogs.length === 0
                ? <div className="p-3 text-xs text-muted-foreground">暂无后端日志，点击刷新按钮加载</div>
                : backendLogs.map((log, index) => (
                    <div key={index} className="grid grid-cols-[160px_70px_90px_minmax(0,1fr)] gap-2 border-b px-3 py-2 text-xs last:border-b-0">
                      <span className="text-muted-foreground">{log.timestamp}</span>
                      <span>{log.level}</span>
                      <span>{log.category}</span>
                      <span className="truncate">{log.message}</span>
                    </div>
                  ))}
          </div>
        </div>
      ) : null}
    </section>
  );
}

function SettingRow({ icon, title, description }: { icon: React.ReactNode; title: string; description: string }) {
  return (
    <div className="rounded-md border bg-background p-3">
      <div className="flex items-center gap-2 text-sm font-medium">
        {icon}
        {title}
      </div>
      <div className="mt-1 text-xs text-muted-foreground">{description}</div>
    </div>
  );
}
