import { RotateCcw, TerminalSquare, X } from "lucide-react";
import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { Button } from "@/components/ui/button";
import { isTauriRuntime, resizeTerminalSession, writeTerminalInput } from "@/lib/tauri";
import { getBufferedOutput, registerTerminalWriter, unregisterTerminalWriter } from "@/terminal/terminal-registry";
import { useAppStore } from "@/stores/app-store";
import type { TerminalTab } from "@/types/app";

const CELL_WIDTH_FALLBACK = 9;
const CELL_HEIGHT_FALLBACK = 20;

export function TerminalTabs({ workspaceVisible }: { workspaceVisible: boolean }) {
  const tabs = useAppStore((state) => state.terminalTabs);
  const activeTabId = useAppStore((state) => state.activeTerminalTabId);
  const closeTerminal = useAppStore((state) => state.closeTerminal);
  const reconnectTerminal = useAppStore((state) => state.reconnectTerminal);
  const selectedServerId = useAppStore((state) => state.selectedServerId);
  const openTerminal = useAppStore((state) => state.openTerminal);
  const setActiveTerminalTab = useAppStore((state) => state.setActiveTerminalTab);
  const activeTab = tabs.find((tab) => tab.id === activeTabId) ?? tabs[0] ?? null;

  return (
    <section className="flex h-full min-h-0 flex-col overflow-hidden bg-card">
      <div className="flex h-10 shrink-0 items-center justify-between border-b px-3">
        <div className="flex items-center gap-2">
          <TerminalSquare size={16} />
          <span className="text-sm font-medium">SSH 终端</span>
          {activeTab ? <span className="rounded bg-muted px-2 py-0.5 text-xs text-muted-foreground">{statusText(activeTab.status)}</span> : null}
        </div>
        <div className="flex items-center gap-1">
          <Button variant="ghost" size="icon" title="重连" onClick={() => activeTab && void reconnectTerminal(activeTab.id)} disabled={!activeTab}>
            <RotateCcw size={15} />
          </Button>
          <Button variant="ghost" size="icon" title="关闭" onClick={() => activeTab && void closeTerminal(activeTab.id)} disabled={!activeTab}>
            <X size={15} />
          </Button>
        </div>
      </div>
      <div className="flex h-9 shrink-0 items-center gap-1 overflow-x-auto border-b bg-background px-2">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setActiveTerminalTab(tab.id)}
            className={`h-7 rounded px-2 text-xs ${tab.id === activeTab?.id ? "bg-muted" : "hover:bg-muted"}`}
          >
            {tab.serverName}
          </button>
        ))}
        <Button variant="outline" size="sm" onClick={() => selectedServerId && void openTerminal(selectedServerId)} disabled={!selectedServerId}>
          新建终端
        </Button>
      </div>
      <div className="relative min-h-0 flex-1 overflow-hidden bg-[#101418]">
        {tabs.map((tab) => (
          <TerminalPane
            key={tab.id}
            tab={tab}
            selected={tab.id === activeTab?.id}
            workspaceVisible={workspaceVisible}
          />
        ))}
        {!activeTab ? (
          <div className="flex h-full items-center justify-center text-sm text-slate-400">请选择服务器并打开一个 SSH 终端标签页。</div>
        ) : null}
      </div>
    </section>
  );
}

function TerminalPane({
  tab,
  selected,
  workspaceVisible,
}: {
  tab: TerminalTab;
  selected: boolean;
  workspaceVisible: boolean;
}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const statusRef = useRef(tab.status);
  const activateRef = useRef<() => void>(() => {});

  useEffect(() => {
    statusRef.current = tab.status;
  }, [tab.status]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const term = new Terminal({
      cursorBlink: true,
      fontFamily: "JetBrains Mono, Cascadia Mono, Consolas, monospace",
      fontSize: 13,
      lineHeight: 1.25,
      convertEol: true,
      scrollback: 2000,
      theme: {
        background: "#101418",
        foreground: "#d8dee9",
        cursor: "#f5b971",
        selectionBackground: "#3d4f63",
      },
    });

    let resizeFrame: number | null = null;
    const scheduleResize = () => {
      if (resizeFrame !== null) return;
      resizeFrame = window.requestAnimationFrame(() => {
        resizeFrame = null;
        resizeToContainer(term, container, tab.id, () => statusRef.current === "connecting" || statusRef.current === "connected");
      });
    };
    activateRef.current = () => {
      scheduleResize();
      window.requestAnimationFrame(() => {
        if (term.rows > 0) term.refresh(0, term.rows - 1);
      });
    };

    term.open(container);
    scheduleResize();
    const initialOutput = getBufferedOutput(tab.id) || tab.output;
    if (initialOutput) term.write(initialOutput);

    registerTerminalWriter(tab.id, (data) => {
      try {
        term.write(data);
      } catch {
        // A backend event can arrive just after xterm has been disposed.
      }
    });

    const inputDisposable = term.onData((data) => {
      if (!isTauriRuntime()) {
        term.write(data === "\r" ? "\r\n" : data);
        return;
      }
      if (statusRef.current === "closed" || statusRef.current === "error") return;
      void writeTerminalInput(tab.id, data).catch(() => {});
    });
    const resizeObserver = new ResizeObserver(scheduleResize);
    resizeObserver.observe(container);

    return () => {
      if (resizeFrame !== null) window.cancelAnimationFrame(resizeFrame);
      activateRef.current = () => {};
      inputDisposable.dispose();
      resizeObserver.disconnect();
      unregisterTerminalWriter(tab.id);
      term.dispose();
    };
  }, [tab.id, tab.output]);

  useEffect(() => {
    if (selected && workspaceVisible) activateRef.current();
  }, [selected, workspaceVisible]);

  return <div ref={containerRef} className={selected ? "terminal-shell absolute inset-2 overflow-hidden" : "hidden"} />;
}

function resizeToContainer(term: Terminal, container: HTMLDivElement, sessionId: string, canNotifyBackend: () => boolean) {
  const width = container.clientWidth;
  const height = container.clientHeight;
  if (width <= 0 || height <= 0) return;

  const rowsEl = container.querySelector(".xterm-rows") as HTMLElement | null;
  let cellW = CELL_WIDTH_FALLBACK;
  let cellH = CELL_HEIGHT_FALLBACK;
  if (rowsEl && term.rows > 0 && term.cols > 0) {
    const measuredWidth = rowsEl.clientWidth / term.cols;
    const measuredHeight = rowsEl.clientHeight / term.rows;
    if (measuredWidth > 0) cellW = measuredWidth;
    if (measuredHeight > 0) cellH = measuredHeight;
  }

  const cols = Math.max(20, Math.floor(width / cellW));
  const rows = Math.max(5, Math.floor(height / cellH));
  if (term.cols !== cols || term.rows !== rows) {
    term.resize(cols, rows);
    if (canNotifyBackend()) void resizeTerminalSession(sessionId, cols, rows).catch(() => {});
  }
}

function statusText(status: string) {
  switch (status) {
    case "connecting":
      return "连接中";
    case "connected":
      return "已连接";
    case "closed":
      return "已关闭";
    case "error":
      return "错误";
    default:
      return status;
  }
}
