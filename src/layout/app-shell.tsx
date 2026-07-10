import { FileSearch, Info, PanelRightOpen, Settings, TerminalSquare, Waypoints } from "lucide-react";
import { RemoteFileManager } from "@/file-manager/remote-file-manager";
import { MonitorPanel } from "@/monitor/monitor-panel";
import { ServerPanel } from "@/server/server-panel";
import { SettingsPanel } from "@/settings/settings-panel";
import { StatusBar } from "@/layout/status-bar";
import { TopBar } from "@/layout/top-bar";
import { TransferPanel } from "@/transfer/transfer-panel";
import { TerminalTabs } from "@/terminal/terminal-tabs";
import { TunnelPanel } from "@/tunnel/tunnel-panel";
import { useAppStore } from "@/stores/app-store";

export function AppShell() {
  const activeView = useAppStore((state) => state.activeView);
  const setActiveView = useAppStore((state) => state.setActiveView);

  return (
    <div className="flex h-full min-h-0 overflow-hidden bg-background text-foreground">
      <ServerPanel />
      <main className="flex min-w-0 flex-1 flex-col overflow-hidden">
        <TopBar />
        <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_320px] overflow-hidden border-t">
          <section className="flex min-w-0 flex-col overflow-hidden">
            <div className="flex h-10 shrink-0 items-center gap-1 border-b bg-card px-3">
              <TabButton icon={<FileSearch size={16} />} label="SFTP 文件" active={activeView === "files"} onClick={() => setActiveView("files")} />
              <TabButton icon={<TerminalSquare size={16} />} label="终端" active={activeView === "terminal"} onClick={() => setActiveView("terminal")} />
              <TabButton icon={<Waypoints size={16} />} label="隧道" active={activeView === "tunnels"} onClick={() => setActiveView("tunnels")} />
              <TabButton icon={<Settings size={16} />} label="设置" active={activeView === "settings"} onClick={() => setActiveView("settings")} />
              <TabButton icon={<Info size={16} />} label="关于" active={activeView === "about"} onClick={() => setActiveView("about")} />
            </div>
            <div className="relative min-h-0 flex-1 overflow-hidden">
              <div className={`absolute inset-0 ${activeView === "terminal" ? "visible" : "invisible pointer-events-none"}`}>
                <TerminalTabs workspaceVisible={activeView === "terminal"} />
              </div>
              {activeView === "files" ? <RemoteFileManager /> : null}
              {activeView === "tunnels" ? <TunnelPanel /> : null}
              {activeView === "settings" ? <SettingsPanel full /> : null}
              {activeView === "about" ? <AboutPanel /> : null}
            </div>
          </section>
          <aside className="min-h-0 overflow-hidden border-l bg-card">
            <div className="flex h-10 items-center gap-2 border-b px-3 text-sm font-medium">
              <PanelRightOpen size={16} />
              任务与设置
            </div>
            <div className="grid h-[calc(100%-2.5rem)] grid-rows-[auto_minmax(0,1fr)_260px]">
              <TransferPanel />
              <MonitorPanel />
              <SettingsPanel />
            </div>
          </aside>
        </div>
        <StatusBar />
      </main>
    </div>
  );
}

function TabButton({ icon, label, active = false, onClick }: { icon: React.ReactNode; label: string; active?: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className={`inline-flex h-8 items-center gap-2 rounded-md px-3 text-sm ${
        active ? "bg-muted text-foreground" : "text-muted-foreground hover:bg-muted"
      }`}
    >
      {icon}
      {label}
    </button>
  );
}

function AboutPanel() {
  return (
    <section className="h-full overflow-auto bg-background p-6">
      <div className="max-w-3xl">
        <h1 className="text-2xl font-semibold">ALAX SSH Manager</h1>
        <p className="mt-2 text-muted-foreground">ALAX SSH 管理器，面向 Windows 优先的现代化 SSH/SFTP 桌面管理工具。</p>
        <div className="mt-6 grid gap-3 md:grid-cols-2">
          {[
            ["安全", "密码、私钥、passphrase 只通过系统凭据存储引用，不写入 SQLite。"],
            ["稳定", "耗时操作通过后端命令和任务队列承载，UI 保持响应。"],
            ["可维护", "服务器、终端、SFTP、传输、日志模块分层组织。"],
            ["跨平台", "基于 Tauri + React + Rust，保留 macOS/Linux 兼容空间。"],
          ].map(([title, description]) => (
            <div key={title} className="rounded-md border bg-card p-4">
              <div className="font-medium">{title}</div>
              <div className="mt-1 text-sm text-muted-foreground">{description}</div>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
