import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { AppShell } from "@/layout/app-shell";
import { useTheme } from "@/hooks/use-theme";
import { closeTerminalSession, initializeDatabase, isTauriRuntime, loadInitialSnapshot } from "@/lib/tauri";
import { writeTerminalOutput } from "@/terminal/terminal-registry";
import { useAppStore } from "@/stores/app-store";
import type { TerminalTab, TransferTask } from "@/types/app";

interface TerminalOutputPayload {
  sessionId: string;
  data: string;
}

interface TerminalStatusPayload {
  sessionId: string;
  status: TerminalTab["status"];
  message?: string | null;
}

interface TransferProgressPayload {
  id: string;
  status: TransferTask["status"];
  progress: number;
  speed: number;
  errorMessage?: string | null;
}

export function App() {
  const { theme } = useTheme();
  const setSnapshot = useAppStore((state) => state.setSnapshot);
  const setRuntimeStatus = useAppStore((state) => state.setRuntimeStatus);
  const updateTerminalStatus = useAppStore((state) => state.updateTerminalStatus);
  const updateTransfer = useAppStore((state) => state.updateTransfer);

  useEffect(() => {
    document.documentElement.classList.toggle("dark", theme === "dark");
  }, [theme]);

  useEffect(() => {
    async function boot() {
      try {
        await initializeDatabase();
        const snapshot = await loadInitialSnapshot();
        setSnapshot(snapshot);
        setRuntimeStatus("ready");
      } catch (error) {
        console.error(error);
        setRuntimeStatus("offline");
      }
    }

    void boot();
  }, [setRuntimeStatus, setSnapshot]);

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    let disposed = false;
    const unlisteners: Array<() => void> = [];

    async function bindEvents() {
      unlisteners.push(
        await listen<TerminalOutputPayload>("terminal-output", (event) => {
          writeTerminalOutput(event.payload.sessionId, event.payload.data);
        }),
      );
      unlisteners.push(
        await listen<TerminalStatusPayload>("terminal-status", (event) => {
          if (event.payload.message) {
            writeTerminalOutput(event.payload.sessionId, `\r\n[${event.payload.message}]\r\n`);
          }
          updateTerminalStatus(event.payload.sessionId, event.payload.status, event.payload.message);
          if (event.payload.status === "closed" || event.payload.status === "error") {
            void closeTerminalSession(event.payload.sessionId).catch(() => {});
          }
        }),
      );
      unlisteners.push(
        await listen<TransferProgressPayload>("transfer-progress", (event) => {
          updateTransfer(event.payload.id, {
            status: event.payload.status,
            progress: event.payload.progress,
            speed: event.payload.speed,
            errorMessage: event.payload.errorMessage ?? undefined,
          });
        }),
      );

      if (disposed) {
        unlisteners.splice(0).forEach((unlisten) => unlisten());
      }
    }

    void bindEvents();
    return () => {
      disposed = true;
      unlisteners.splice(0).forEach((unlisten) => unlisten());
    };
  }, [updateTerminalStatus, updateTransfer]);

  return <AppShell />;
}
