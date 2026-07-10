import { RotateCcw, XCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cancelTransferTask, retryTransferTask } from "@/lib/tauri";
import { formatBytes } from "@/lib/utils";
import { useAppStore } from "@/stores/app-store";

export function TransferPanel() {
  const transfers = useAppStore((state) => state.transfers);
  const cancelTransfer = useAppStore((state) => state.cancelTransfer);
  const retryTransfer = useAppStore((state) => state.retryTransfer);
  const addLog = useAppStore((state) => state.addLog);

  async function cancel(taskId: string) {
    cancelTransfer(taskId);
    try {
      await cancelTransferTask(taskId);
    } catch (error) {
      addLog({ level: "warn", category: "transfer", message: error instanceof Error ? error.message : String(error) });
    }
  }

  async function retry(taskId: string) {
    retryTransfer(taskId);
    try {
      await retryTransferTask(taskId);
    } catch (error) {
      addLog({ level: "error", category: "transfer", message: error instanceof Error ? error.message : String(error) });
    }
  }

  return (
    <section className="min-h-0 overflow-auto border-b p-3">
      <div className="mb-3 flex items-center justify-between">
        <div>
          <div className="text-sm font-medium">传输队列</div>
          <div className="text-xs text-muted-foreground">后台任务会持续回传真实进度</div>
        </div>
      </div>
      <div className="space-y-2">
        {transfers.length === 0 ? <div className="rounded-md border bg-background p-3 text-xs text-muted-foreground">暂无传输任务</div> : null}
        {transfers.map((task) => (
          <div key={task.id} className="rounded-md border bg-background p-3">
            <div className="flex items-start justify-between gap-2">
              <div className="min-w-0">
                <div className="truncate text-sm font-medium">{task.fileName}</div>
                <div className="truncate text-xs text-muted-foreground">
                  {task.type === "upload" ? "上传" : "下载"} · {task.serverName}
                </div>
              </div>
              <div className="flex">
                <Button variant="ghost" size="icon" title="重试" onClick={() => void retry(task.id)} disabled={task.status === "running"}>
                  <RotateCcw size={14} />
                </Button>
                <Button variant="ghost" size="icon" title="取消" onClick={() => void cancel(task.id)} disabled={task.status !== "running"}>
                  <XCircle size={14} />
                </Button>
              </div>
            </div>
            <div className="mt-3 h-1.5 overflow-hidden rounded bg-muted">
              <div className="h-full rounded bg-primary transition-[width]" style={{ width: `${Math.max(0, Math.min(100, task.progress))}%` }} />
            </div>
            <div className="mt-2 flex justify-between gap-2 text-xs text-muted-foreground">
              <span className="truncate">{statusText(task.status, task.errorMessage)}</span>
              <span>{task.speed > 0 && task.status === "running" ? `${formatBytes(task.speed)}/s` : `${Math.round(task.progress)}%`}</span>
            </div>
          </div>
        ))}
      </div>
    </section>
  );
}

function statusText(status: string, errorMessage?: string) {
  switch (status) {
    case "running":
      return "传输中";
    case "done":
      return "已完成";
    case "failed":
      return errorMessage ?? "失败";
    case "cancelled":
      return "已取消";
    case "queued":
      return "排队中";
    default:
      return status;
  }
}
