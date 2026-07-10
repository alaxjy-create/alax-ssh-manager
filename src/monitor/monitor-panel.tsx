import { Activity, Cpu, HardDrive, Thermometer, Wifi } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { getServerStats } from "@/lib/tauri";
import { useAppStore } from "@/stores/app-store";

interface ServerStats {
  cpuUsage: number;
  memoryUsed: number;
  memoryTotal: number;
  swapUsed: number;
  swapTotal: number;
  disks: { mount: string; used: number; total: number }[];
  networks: { name: string; rxBytes: number; txBytes: number }[];
  temperature: number | null;
  uptime: number;
  loadAvg1: number;
  loadAvg5: number;
  loadAvg15: number;
}

export function MonitorPanel() {
  const selectedServerId = useAppStore((state) => state.selectedServerId);
  const servers = useAppStore((state) => state.servers);
  const addLog = useAppStore((state) => state.addLog);
  const selectedServer = servers.find((s) => s.id === selectedServerId) ?? null;

  const [stats, setStats] = useState<ServerStats | null>(null);
  const [monitoring, setMonitoring] = useState(false);
  const [netSpeed, setNetSpeed] = useState<{ name: string; rxSpeed: number; txSpeed: number }[]>([]);
  const previousNetworkRef = useRef<{ at: number; values: { name: string; rx: number; tx: number }[] } | null>(null);
  const fetchingRef = useRef(false);

  const fetchStats = useCallback(async () => {
    if (!selectedServerId || !monitoring || fetchingRef.current || document.hidden) return;
    fetchingRef.current = true;
    try {
      const result = await getServerStats(selectedServerId);
      const now = Date.now();
      const previous = previousNetworkRef.current;
      if (previous) {
        const elapsedSeconds = Math.max((now - previous.at) / 1000, 0.001);
        const speeds = result.networks.map((net) => {
          const prev = previous.values.find((item) => item.name === net.name);
          if (prev) {
            return {
              name: net.name,
              rxSpeed: Math.max(0, net.rxBytes - prev.rx) / elapsedSeconds,
              txSpeed: Math.max(0, net.txBytes - prev.tx) / elapsedSeconds,
            };
          }
          return { name: net.name, rxSpeed: 0, txSpeed: 0 };
        });
        setNetSpeed(speeds);
      }
      previousNetworkRef.current = {
        at: now,
        values: result.networks.map((network) => ({ name: network.name, rx: network.rxBytes, tx: network.txBytes })),
      };
      setStats(result);
    } catch (err) {
      addLog({ level: "error", category: "monitor", message: err instanceof Error ? err.message : String(err) });
    } finally {
      fetchingRef.current = false;
    }
  }, [selectedServerId, monitoring, addLog]);

  useEffect(() => {
    if (!selectedServerId || !monitoring) {
      setStats(null);
      previousNetworkRef.current = null;
      setNetSpeed([]);
      return;
    }
    previousNetworkRef.current = null;
    void fetchStats();
    const interval = window.setInterval(() => void fetchStats(), 10_000);
    const onVisibilityChange = () => {
      if (!document.hidden) void fetchStats();
    };
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      window.clearInterval(interval);
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [selectedServerId, monitoring, fetchStats]);

  useEffect(() => {
    setMonitoring(false);
  }, [selectedServerId]);

  if (!selectedServer) {
    return (
      <div className="flex h-full items-center justify-center p-3 text-xs text-muted-foreground">
        请选择一个服务器查看监控信息
      </div>
    );
  }

  if (!monitoring) {
    return (
      <div className="flex h-full items-center justify-center p-3 text-xs text-muted-foreground">
        <Button variant="outline" size="sm" onClick={() => setMonitoring(true)}>
          <Activity size={14} />
          启动监控
        </Button>
      </div>
    );
  }

  if (!stats) {
    return <div className="flex h-full items-center justify-center p-3 text-xs text-muted-foreground">正在获取服务器信息...</div>;
  }

  const memPercent = stats.memoryTotal > 0 ? (stats.memoryUsed / stats.memoryTotal) * 100 : 0;

  function formatBytes(bytes: number): string {
    if (bytes === 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB", "TB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + " " + sizes[i];
  }

  function formatSpeed(bytesPerSec: number): string {
    if (bytesPerSec < 0) return "0 B/s";
    const k = 1024;
    const sizes = ["B/s", "KB/s", "MB/s", "GB/s"];
    const i = Math.floor(Math.log(Math.max(bytesPerSec, 1)) / Math.log(k));
    if (i >= sizes.length) return formatBytes(bytesPerSec) + "/s";
    return parseFloat((bytesPerSec / Math.pow(k, i)).toFixed(1)) + " " + sizes[i];
  }

  function formatUptime(seconds: number): string {
    const days = Math.floor(seconds / 86400);
    const hours = Math.floor((seconds % 86400) / 3600);
    const mins = Math.floor((seconds % 3600) / 60);
    if (days > 0) return `${days}d ${hours}h ${mins}m`;
    if (hours > 0) return `${hours}h ${mins}m`;
    return `${mins}m`;
  }

  const SectionHeader = ({ icon, label }: { icon: React.ReactNode; label: string }) => (
    <div className="mb-1 flex items-center gap-1 text-[11px] font-medium text-muted-foreground">
      {icon}
      {label}
    </div>
  );

  return (
    <div className="h-full overflow-auto p-3 text-xs">
      <div className="mb-3">
        <div className="mb-1 flex items-center gap-1 text-[11px] font-medium text-muted-foreground">
          <Activity size={12} />
          {selectedServer.name}
        </div>
        <div className="text-[11px] text-muted-foreground">
          运行时间 {formatUptime(stats.uptime)}
        </div>
      </div>

      <div className="mb-3">
        <SectionHeader icon={<Cpu size={12} />} label="CPU" />
        <div className="flex items-center gap-2">
          <div className="h-2 flex-1 overflow-hidden rounded-full bg-muted">
            <div className="h-full rounded-full bg-primary transition-all" style={{ width: `${stats.cpuUsage}%` }} />
          </div>
          <span className="w-10 text-right text-[11px] tabular-nums">{stats.cpuUsage.toFixed(1)}%</span>
        </div>
        <div className="mt-1 text-[11px] text-muted-foreground">
          负载 {stats.loadAvg1.toFixed(2)} / {stats.loadAvg5.toFixed(2)} / {stats.loadAvg15.toFixed(2)}
        </div>
      </div>

      <div className="mb-3">
        <SectionHeader icon={<HardDrive size={12} />} label="内存" />
        <div className="flex items-center gap-2">
          <div className="h-2 flex-1 overflow-hidden rounded-full bg-muted">
            <div className={`h-full rounded-full transition-all ${memPercent > 80 ? "bg-red-500" : "bg-blue-500"}`} style={{ width: `${memPercent}%` }} />
          </div>
          <span className="w-22 text-right text-[11px] tabular-nums">{formatBytes(stats.memoryUsed)} / {formatBytes(stats.memoryTotal)}</span>
        </div>
        {stats.swapTotal > 0 ? (
          <div className="mt-1 flex items-center gap-2">
            <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
              <div className="h-full rounded-full bg-yellow-500 transition-all" style={{ width: `${(stats.swapUsed / stats.swapTotal) * 100}%` }} />
            </div>
            <span className="w-22 text-right text-[11px] tabular-nums text-muted-foreground">{formatBytes(stats.swapUsed)} / {formatBytes(stats.swapTotal)}</span>
          </div>
        ) : null}
      </div>

      {stats.disks.length > 0 ? (
        <div className="mb-3">
          <SectionHeader icon={<HardDrive size={12} />} label="磁盘" />
          {stats.disks.map((disk) => {
            const pct = disk.total > 0 ? (disk.used / disk.total) * 100 : 0;
            return (
              <div key={disk.mount} className="mb-1.5">
                <div className="flex items-center justify-between text-[11px] text-muted-foreground">
                  <span>{disk.mount}</span>
                  <span>{formatBytes(disk.used)} / {formatBytes(disk.total)}</span>
                </div>
                <div className="mt-0.5 h-1.5 overflow-hidden rounded-full bg-muted">
                  <div className={`h-full rounded-full transition-all ${pct > 85 ? "bg-red-500" : "bg-green-500"}`} style={{ width: `${pct}%` }} />
                </div>
              </div>
            );
          })}
        </div>
      ) : null}

      {stats.networks.length > 0 ? (
        <div className="mb-3">
          <SectionHeader icon={<Wifi size={12} />} label="网络" />
          {stats.networks.slice(0, 3).map((net) => {
            const speed = netSpeed.find((s) => s.name === net.name);
            return (
              <div key={net.name} className="mb-1 flex justify-between text-[11px]">
                <span className="text-muted-foreground">{net.name}</span>
                <span className="tabular-nums">
                  {speed ? `↓${formatSpeed(speed.rxSpeed)} ↑${formatSpeed(speed.txSpeed)}` : "..."}
                </span>
              </div>
            );
          })}
        </div>
      ) : null}

      {stats.temperature != null ? (
        <div>
          <SectionHeader icon={<Thermometer size={12} />} label="温度" />
          <div className="flex items-center gap-2">
            <div className="h-2 flex-1 overflow-hidden rounded-full bg-muted">
              <div className={`h-full rounded-full transition-all ${stats.temperature > 80 ? "bg-red-500" : stats.temperature > 60 ? "bg-yellow-500" : "bg-blue-500"}`} style={{ width: `${Math.min(stats.temperature / 100 * 100, 100)}%` }} />
            </div>
            <span className="w-14 text-right text-[11px] tabular-nums">{stats.temperature.toFixed(1)}°C</span>
          </div>
        </div>
      ) : null}
    </div>
  );
}
