import { Activity, Database, LockKeyhole } from "lucide-react";
import { useAppStore } from "@/stores/app-store";

export function StatusBar() {
  const activePath = useAppStore((state) => state.activePath);
  const files = useAppStore((state) => state.files);

  return (
    <footer className="flex h-8 shrink-0 items-center justify-between border-t bg-card px-3 text-xs text-muted-foreground">
      <div className="flex items-center gap-4">
        <span className="inline-flex items-center gap-1.5">
          <Activity size={13} />
          当前路径：{activePath}
        </span>
        <span>已显示 {files.length} 项</span>
      </div>
      <div className="flex items-center gap-4">
        <span className="inline-flex items-center gap-1.5">
          <LockKeyhole size={13} />
          凭据不进入 SQLite
        </span>
        <span className="inline-flex items-center gap-1.5">
          <Database size={13} />
          SQLite 初始化已预留
        </span>
      </div>
    </footer>
  );
}
