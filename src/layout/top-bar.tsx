import { Moon, Plus, RefreshCw, Search, Sun, Upload } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useTheme } from "@/hooks/use-theme";
import { useAppStore } from "@/stores/app-store";

export function TopBar() {
  const { theme, setTheme } = useTheme();
  const runtimeStatus = useAppStore((state) => state.runtimeStatus);
  const searchTerm = useAppStore((state) => state.searchTerm);
  const setSearchTerm = useAppStore((state) => state.setSearchTerm);
  const setActiveView = useAppStore((state) => state.setActiveView);

  return (
    <header className="flex h-14 shrink-0 items-center gap-3 bg-card px-4">
      <div className="min-w-[220px]">
        <div className="text-sm font-semibold">ALAX SSH Manager</div>
        <div className="text-xs text-muted-foreground">ALAX SSH 管理器</div>
      </div>
      <div className="flex h-9 flex-1 items-center gap-2 rounded-md border bg-background px-3 text-sm text-muted-foreground">
        <Search size={16} />
        <input
          className="w-full bg-transparent outline-none"
          value={searchTerm}
          onChange={(event) => setSearchTerm(event.target.value)}
          placeholder="搜索服务器、标签或远程路径"
        />
      </div>
      <div className="flex items-center gap-2">
        <span className="rounded bg-muted px-2 py-1 text-xs text-muted-foreground">
          {runtimeStatus === "ready" ? "后端就绪" : runtimeStatus === "offline" ? "浏览器预览" : "启动中"}
        </span>
        <Button variant="outline" size="sm">
          <RefreshCw size={15} />
          刷新
        </Button>
        <Button variant="outline" size="sm">
          <Upload size={15} />
          上传
        </Button>
        <Button size="sm" onClick={() => setActiveView("files")}>
          <Plus size={15} />
          新建服务器
        </Button>
        <Button variant="ghost" size="icon" onClick={() => setTheme(theme === "dark" ? "light" : "dark")} title="切换主题">
          {theme === "dark" ? <Sun size={17} /> : <Moon size={17} />}
        </Button>
      </div>
    </header>
  );
}
