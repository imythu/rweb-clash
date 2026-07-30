import { useState, useEffect, useCallback, useRef, type ReactNode } from 'react';
import { 
  ArrowUp, 
  ArrowDown, 
  Activity, 
  Globe2, 
  Zap,
  Shield,
  Cpu,
  Compass,
  ZapOff,
  Network,
  Radio,
  AlertCircle,
  Clock3,
  Loader2,
  Power,
  Play,
  RotateCw,
  Square,
  Info,
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { useToast } from './toast-context';
import { api, ApiError, type Connection, type CoreStatus, type Egress, type SystemConfig, type Traffic } from '@/lib/api';
import { usePageActivity } from '@/lib/usePageActivity';

const REALTIME_POLL_MS = 5000;
const STATUS_POLL_MS = 15000;
const EGRESS_POLL_MS = 300000;
const EGRESS_CACHE_KEY = 'rweb-clash:egress:v1';
const EMPTY_EGRESS: Egress = { ip: null, provider: null, country: null, source: null };

const readCachedEgress = (): Egress => {
  try {
    const cached = JSON.parse(localStorage.getItem(EGRESS_CACHE_KEY) ?? 'null') as Partial<Egress> | null;
    if (!cached || typeof cached.ip !== 'string' || cached.ip.length === 0) return EMPTY_EGRESS;
    return {
      ip: cached.ip,
      provider: typeof cached.provider === 'string' ? cached.provider : null,
      country: typeof cached.country === 'string' ? cached.country : null,
      source: typeof cached.source === 'string' ? cached.source : null,
    };
  } catch {
    return EMPTY_EGRESS;
  }
};

const writeCachedEgress = (value: Egress) => {
  try {
    localStorage.setItem(EGRESS_CACHE_KEY, JSON.stringify(value));
  } catch {
    // Storage may be unavailable in private or restricted webviews.
  }
};

const DEFAULT_SYSTEM_CONFIG: SystemConfig = {
  allow_lan: false,
  ipv6: true,
  log_level: 'info',
  mixed_port: 7890,
  external_controller: '127.0.0.1:9090',
  external_controller_enabled: true,
  secret: '',
  dns_enabled: true,
  dns_mode: 'fake-ip',
  dns_nameservers: [],
  dns_fallback: [],
  dns_fake_ip_filter: [],
  dns_nameserver_policy: {},
  dns_hosts: {},
  store_selected: true,
  unified_delay: true,
  tcp_concurrent: false,
  tun: false,
  system_proxy: false,
  mode: 'rule',
  auto_start: false,
};

const StatCard = ({
  title,
  value,
  unit,
  icon: Icon,
  color,
}: {
  title: string;
  value: string | number;
  unit: string;
  icon: LucideIcon;
  color?: 'blue' | 'green';
}) => (
  <div className="bg-card border rounded-2xl p-4 md:p-6 shadow-sm relative overflow-hidden group hover:border-primary/20 hover:shadow-md transition-all">
    <div className="flex justify-between items-start relative z-10 text-left">
      <div className="min-w-0">
        <p className="text-[10px] md:text-xs font-black text-muted-foreground mb-1 uppercase tracking-tighter truncate">{title}</p>
        <div className="flex items-baseline gap-1">
          <span className="text-2xl md:text-3xl font-black tracking-tighter">{value}</span>
          <span className="text-[10px] md:text-xs text-muted-foreground font-bold">{unit}</span>
        </div>
      </div>
      <Icon className={cn("size-5 md:size-6 opacity-20 transition-transform group-hover:scale-110", color === 'blue' ? "text-blue-500" : "text-green-500")} />
    </div>
  </div>
);

const MasterSwitch = ({
  icon: Icon,
  label,
  active,
  onClick,
  accessory,
}: {
  icon: LucideIcon;
  label: string;
  active?: boolean;
  onClick: () => void;
  accessory?: ReactNode;
}) => (
  <div 
    onClick={onClick}
    className="flex-1 group relative flex items-center justify-between p-4 md:p-6 rounded-[1.5rem] md:rounded-[2.5rem] transition-all duration-300 bg-card border hover:border-primary/30 hover:shadow-md cursor-pointer shadow-sm active:scale-[0.98]"
  >
    <div className="flex items-center gap-3 md:gap-4 text-left">
      <div className={cn(
        "size-10 md:size-14 rounded-xl md:rounded-[1.25rem] flex items-center justify-center transition-all duration-500",
        active ? "bg-primary/10 text-primary" : "bg-muted text-muted-foreground"
      )}>
        <Icon className="size-5 md:size-7" />
      </div>
      <div className="flex flex-col">
        <div className="flex items-center gap-1.5 text-sm md:text-lg font-black uppercase tracking-tight">
          <span>{label}</span>
          {accessory}
        </div>
        <span className={cn("text-[10px] md:text-xs font-bold uppercase", active ? "text-green-500" : "text-muted-foreground")}>
          {active ? 'Active' : 'Disabled'}
        </span>
      </div>
    </div>

    <div className={cn(
      "w-12 md:w-16 h-6 md:h-8 rounded-full p-1 transition-all duration-500 relative",
      active ? "bg-green-500 shadow-[0_0_15px_rgba(34,197,94,0.3)]" : "bg-muted-foreground/20 shadow-inner"
    )}>
      <div className={cn(
        "size-4 md:size-6 bg-white rounded-full transition-all duration-500 shadow-lg",
        active ? "translate-x-6 md:translate-x-8" : "translate-x-0"
      )} />
    </div>
  </div>
);

const TunHelpBadge = () => {
  const [hovered, setHovered] = useState(false);
  const [pinned, setPinned] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const open = hovered || pinned;

  useEffect(() => {
    if (!pinned) return;
    const closeOnOutsideClick = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setPinned(false);
    };
    document.addEventListener('pointerdown', closeOnOutsideClick);
    return () => document.removeEventListener('pointerdown', closeOnOutsideClick);
  }, [pinned]);

  return (
    <div
      ref={rootRef}
      className="relative normal-case"
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onClick={(event) => {
        event.stopPropagation();
        setPinned((value) => !value);
      }}
      onKeyDown={(event) => {
        if (event.key === 'Escape') setPinned(false);
      }}
    >
      <button
        type="button"
        aria-label="查看 TUN 开启条件"
        aria-expanded={open}
        className="flex size-5 items-center justify-center rounded-full border border-primary/30 bg-primary/10 text-primary hover:bg-primary/20"
      >
        <Info className="size-3" />
      </button>
      {open && (
        <div
          role="tooltip"
          onClick={(event) => event.stopPropagation()}
          className="absolute left-1/2 top-7 z-50 w-[min(20rem,calc(100vw-2rem))] -translate-x-1/2 rounded-lg border bg-popover p-4 text-left text-popover-foreground shadow-xl xl:left-0 xl:translate-x-0"
        >
          <p className="mb-3 text-sm font-bold">TUN 开启条件</p>
          <div className="space-y-3 text-xs font-normal leading-5 text-muted-foreground">
            <p><strong className="text-foreground">Windows：</strong>需要管理员权限。请以管理员身份重新启动 rweb-clash。</p>
            <p><strong className="text-foreground">Linux：</strong>需要 <code>CAP_NET_ADMIN</code>，以及可读写的 <code>/dev/net/tun</code> 字符设备。容器需添加 <code>NET_ADMIN</code> 并映射该设备；宿主缺少设备时先执行 <code>sudo modprobe tun</code>。</p>
            <p><strong className="text-foreground">macOS：</strong>需要系统管理员授权，并确保系统自带的授权辅助工具可用；开启时请完成系统弹出的授权。</p>
          </div>
        </div>
      )}
    </div>
  );
};

type CoreAction = 'start' | 'stop' | 'restart';

type CoreStateMeta = {
  label: string;
  short: string;
  description: string;
  dot: string;
  badge: string;
};

const coreStateMeta: Record<string, CoreStateMeta> = {
  running: {
    label: '运行中',
    short: 'ON',
    description: '内核正在接管代理流量',
    dot: 'bg-green-500 animate-pulse',
    badge: 'border-green-500/20 bg-green-500/10 text-green-600',
  },
  starting: {
    label: '启动中',
    short: 'START',
    description: '正在拉起 Mihomo 进程',
    dot: 'bg-amber-500 animate-pulse',
    badge: 'border-amber-500/20 bg-amber-500/10 text-amber-600',
  },
  stopping: {
    label: '停止中',
    short: 'STOP',
    description: '正在关闭内核进程',
    dot: 'bg-orange-500 animate-pulse',
    badge: 'border-orange-500/20 bg-orange-500/10 text-orange-600',
  },
  not_running: {
    label: '未运行',
    short: 'OFF',
    description: '内核未启动，流量不会接管',
    dot: 'bg-muted-foreground/50',
    badge: 'border-muted bg-muted/50 text-muted-foreground',
  },
  error: {
    label: '异常',
    short: 'ERR',
    description: '内核启动或运行异常',
    dot: 'bg-red-500',
    badge: 'border-red-500/20 bg-red-500/10 text-red-600',
  },
};

const getCoreStateMeta = (state?: string | null) =>
  coreStateMeta[state ?? 'not_running'] ?? {
    label: state ?? '未知',
    short: 'UNK',
    description: '等待状态同步',
    dot: 'bg-muted-foreground/50',
    badge: 'border-muted bg-muted/50 text-muted-foreground',
  };

const formatStartedAt = (startedAt?: string | null) => {
  if (!startedAt) return '尚未启动';
  const date = new Date(startedAt);
  if (Number.isNaN(date.getTime())) return startedAt;

  return date.toLocaleString('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  });
};

const coreActionMessages: Record<CoreAction, string> = {
  start: '已发送启动指令',
  stop: '已发送停止指令',
  restart: '已发送重启指令',
};

const CoreControl = ({
  status,
  busyAction,
  onAction,
}: {
  status: CoreStatus | null;
  busyAction: CoreAction | null;
  onAction: (action: CoreAction) => void;
}) => {
  const state = status?.state ?? 'unknown';
  const meta = getCoreStateMeta(status?.state);
  const isRunning = state === 'running';
  const isStarting = state === 'starting';
  const isStopping = state === 'stopping';
  const isBusy = busyAction !== null || isStarting || isStopping;
  const canStart = !isRunning && !isStarting && !isStopping;
  const canStop = isRunning || isStarting;
  const canRestart = isRunning;

  return (
    <div className="bg-card border rounded-[1.5rem] md:rounded-[2.5rem] p-4 md:p-6 shadow-sm flex flex-col gap-4">
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0 text-left space-y-2">
          <div className="flex items-center gap-2">
            <span className={cn("size-2.5 rounded-full shrink-0", meta.dot)} />
            <span className={cn("border rounded-full px-2.5 py-1 text-[10px] font-black uppercase tracking-widest", meta.badge)}>
              {meta.label}
            </span>
          </div>
          <div className="space-y-1">
            <p className="text-lg md:text-xl font-black tracking-tight">Mihomo Core</p>
            <p className="text-xs font-bold text-muted-foreground">{meta.description}</p>
          </div>
        </div>
        <div className="size-10 md:size-12 rounded-xl md:rounded-[1.25rem] bg-primary/10 text-primary flex items-center justify-center shrink-0">
          <Cpu className="size-5 md:size-6" />
        </div>
      </div>

      <div className="grid grid-cols-2 gap-x-4 gap-y-2 border-t pt-3 text-left">
        <div className="min-w-0">
          <p className="text-[9px] font-black text-muted-foreground uppercase tracking-widest">PID</p>
          <p className="text-sm font-mono font-black truncate">{status?.pid ?? '-'}</p>
        </div>
        <div className="min-w-0">
          <p className="text-[9px] font-black text-muted-foreground uppercase tracking-widest flex items-center gap-1">
            <Clock3 className="size-3" />
            启动时间
          </p>
          <p className="text-sm font-mono font-black truncate">{formatStartedAt(status?.started_at)}</p>
        </div>
      </div>

      {status?.last_error && (
        <div className="flex items-start gap-2 text-left text-xs font-bold text-red-600 bg-red-500/10 border border-red-500/20 rounded-xl px-3 py-2">
          <AlertCircle className="size-4 shrink-0 mt-0.5" />
          <span className="min-w-0 break-words">{status.last_error}</span>
        </div>
      )}

      <div className="grid grid-cols-3 gap-2">
        <Button
          size="sm"
          className="h-10 rounded-xl text-xs font-black"
          disabled={!canStart || isBusy}
          onClick={() => onAction('start')}
        >
          {busyAction === 'start' ? <Loader2 className="size-4 animate-spin" /> : <Play className="size-4" />}
          启动
        </Button>
        <Button
          size="sm"
          variant="destructive"
          className="h-10 rounded-xl text-xs font-black"
          disabled={!canStop || busyAction !== null || isStopping}
          onClick={() => onAction('stop')}
        >
          {busyAction === 'stop' ? <Loader2 className="size-4 animate-spin" /> : <Square className="size-4" />}
          停止
        </Button>
        <Button
          size="sm"
          variant="outline"
          className="h-10 rounded-xl text-xs font-black"
          disabled={!canRestart || isBusy}
          onClick={() => onAction('restart')}
        >
          {busyAction === 'restart' ? <Loader2 className="size-4 animate-spin" /> : <RotateCw className="size-4" />}
          重启
        </Button>
      </div>
    </div>
  );
};

export const Dashboard = () => {
  const { toast } = useToast();
  const [activeMode, setActiveMode] = useState<SystemConfig['mode']>('rule');
  const [stats, setStats] = useState<Traffic>({ up: 0, down: 0 });
  const [connections, setConnections] = useState<Connection[]>([]);
  const [config, setConfig] = useState<SystemConfig>(DEFAULT_SYSTEM_CONFIG);
  const [coreStatus, setCoreStatus] = useState<CoreStatus | null>(null);
  const [coreAction, setCoreAction] = useState<CoreAction | null>(null);
  const [egress, setEgress] = useState<Egress>(readCachedEgress);
  const [egressLoading, setEgressLoading] = useState(false);
  const realtimeInFlight = useRef(false);
  const configUpdateInFlight = useRef(false);
  const coreActionInFlight = useRef(false);
  const statusRequest = useRef<{ controller: AbortController; generation: number } | null>(null);
  const statusGeneration = useRef(0);
  const egressRequest = useRef<{ controller: AbortController; generation: number } | null>(null);
  const egressGeneration = useRef(0);
  const previousEgressCoreState = useRef<CoreStatus['state'] | null>(null);
  const isPageActive = usePageActivity();

  const fetchRealtime = useCallback(async () => {
    if (!isPageActive || document.hidden || realtimeInFlight.current) return;
    realtimeInFlight.current = true;
    try {
      const [traffic, conns] = await Promise.all([
        api.traffic(),
        api.connections(),
      ]);
      setStats({ up: traffic.up, down: traffic.down });
      setConnections(conns);
    } catch (e) {
      console.error("Fetch realtime error:", e);
    } finally {
      realtimeInFlight.current = false;
    }
  }, [isPageActive]);

  const fetchStatus = useCallback(async (force = false) => {
    if (!isPageActive || document.hidden) return;
    if (statusRequest.current && !force) return;
    statusRequest.current?.controller.abort();
    const generation = ++statusGeneration.current;
    const controller = new AbortController();
    statusRequest.current = { controller, generation };
    try {
      const status = await api.systemStatus(controller.signal);
      if (statusRequest.current?.generation !== generation) return;
      setActiveMode(status.config.mode || 'rule');
      setConfig(status.config);
      setCoreStatus(status.core);
    } catch (e) {
      if (!(e instanceof DOMException && e.name === 'AbortError')) {
        console.error("Fetch status error:", e);
      }
    } finally {
      if (statusRequest.current?.generation === generation) {
        statusRequest.current = null;
      }
    }
  }, [isPageActive]);

  const fetchEgress = useCallback(async (force = false) => {
    if (!isPageActive || document.hidden) return;
    if (egressRequest.current && !force) return;
    egressRequest.current?.controller.abort();
    const generation = ++egressGeneration.current;
    const controller = new AbortController();
    egressRequest.current = { controller, generation };
    setEgressLoading(true);
    try {
      const egressInfo = await api.systemEgress(controller.signal);
      if (egressRequest.current?.generation !== generation) return;
      if (egressInfo.ip) {
        writeCachedEgress(egressInfo);
        setEgress(egressInfo);
      } else {
        setEgress((current) => current.ip ? current : egressInfo);
      }
    } catch (e) {
      if (!(e instanceof DOMException && e.name === 'AbortError')) {
        console.error("Fetch egress error:", e);
      }
    } finally {
      if (egressRequest.current?.generation === generation) {
        egressRequest.current = null;
        setEgressLoading(false);
      }
    }
  }, [isPageActive]);

  useEffect(() => {
    if (!isPageActive) return;

    const fetchAll = () => {
      void fetchRealtime();
      void fetchStatus();
      void fetchEgress();
    };
    const handleVisibilityChange = () => {
      if (!document.hidden) fetchAll();
    };

    queueMicrotask(fetchAll);
    document.addEventListener('visibilitychange', handleVisibilityChange);
    const realtimeTimer = window.setInterval(fetchRealtime, REALTIME_POLL_MS);
    const statusTimer = window.setInterval(fetchStatus, STATUS_POLL_MS);
    const egressTimer = window.setInterval(fetchEgress, EGRESS_POLL_MS);
    return () => {
      statusGeneration.current += 1;
      statusRequest.current?.controller.abort();
      statusRequest.current = null;
      egressGeneration.current += 1;
      egressRequest.current?.controller.abort();
      egressRequest.current = null;
      document.removeEventListener('visibilitychange', handleVisibilityChange);
      window.clearInterval(realtimeTimer);
      window.clearInterval(statusTimer);
      window.clearInterval(egressTimer);
    };
  }, [fetchRealtime, fetchStatus, fetchEgress, isPageActive]);

  useEffect(() => {
    const nextState = coreStatus?.state;
    if (!nextState) return;
    const previousState = previousEgressCoreState.current;
    previousEgressCoreState.current = nextState;
    if (previousState !== null && previousState !== nextState) {
      void fetchEgress(true);
    }
  }, [coreStatus?.state, fetchEgress]);

  const applyConfigUpdate = async (updates: Partial<SystemConfig>) => {
    const nextConfig = await api.patchConfig(updates);
    setConfig(nextConfig);
    if (updates.mode) setActiveMode(updates.mode);
    await Promise.all([fetchStatus(true), fetchEgress(true)]);
    toast('状态同步成功', 'success');
  };

  const updateConfig = async (updates: Partial<SystemConfig>) => {
    if (configUpdateInFlight.current) return;
    configUpdateInFlight.current = true;
    try {
      await applyConfigUpdate(updates);
    } catch (error) {
      toast(error instanceof ApiError ? error.message : '同步失败', 'error');
    } finally {
      configUpdateInFlight.current = false;
    }
  };

  const toggleTun = async () => {
    if (configUpdateInFlight.current) return;
    configUpdateInFlight.current = true;
    try {
      if (!config.tun) await api.checkTun();
      await applyConfigUpdate({ tun: !config.tun });
    } catch (error) {
      toast(error instanceof ApiError ? error.message : 'TUN 状态同步失败', 'error');
    } finally {
      configUpdateInFlight.current = false;
    }
  };

  const runCoreAction = async (action: CoreAction) => {
    if (coreActionInFlight.current) return;
    coreActionInFlight.current = true;
    setCoreAction(action);
    try {
      const nextCore = action === 'start'
        ? await api.startCore()
        : action === 'stop'
          ? await api.stopCore()
          : await api.restartCore();
      previousEgressCoreState.current = nextCore.state;
      setCoreStatus(nextCore);
      toast(coreActionMessages[action], 'success');
      await Promise.all([fetchRealtime(), fetchStatus(true), fetchEgress(true)]);
    } catch (e) {
      console.error("Core action error:", e);
      toast(e instanceof ApiError ? e.message : '内核操作失败', 'error');
    } finally {
      coreActionInFlight.current = false;
      setCoreAction(null);
    }
  };

  const formatSpeed = (bytes: number) => {
    if (bytes > 1024 * 1024) return (bytes / (1024 * 1024)).toFixed(1) + ' MB/s';
    return (bytes / 1024).toFixed(0) + ' KB/s';
  };

  const modes: Array<{ id: SystemConfig['mode']; label: string; icon: LucideIcon }> = [
    { id: 'rule', label: '规则分流', icon: Shield },
    { id: 'global', label: '全局代理', icon: Compass },
    { id: 'direct', label: '绕过直连', icon: ZapOff },
  ];

  return (
    <div className="space-y-6 md:space-y-12 max-w-7xl mx-auto pb-12 animate-in fade-in duration-500">
      
      {/* 1. MASTER CONTROL ROW */}
      <div className="grid grid-cols-1 xl:grid-cols-3 gap-4 md:gap-8 px-1">
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4 md:gap-8 xl:col-span-2">
          <MasterSwitch icon={Shield} label="系统代理" active={config.system_proxy} onClick={() => updateConfig({ system_proxy: !config.system_proxy })} />
          <MasterSwitch icon={Zap} label="TUN 模式" active={config.tun} onClick={() => void toggleTun()} accessory={<TunHelpBadge />} />
          <MasterSwitch icon={Power} label="自动启动" active={config.auto_start} onClick={() => updateConfig({ auto_start: !config.auto_start })} />
        </div>
        <CoreControl status={coreStatus} busyAction={coreAction} onAction={runCoreAction} />
      </div>

      {/* 2. ENGINE SELECTION */}
      <div className="px-1">
        <div className="bg-muted p-1.5 rounded-2xl flex items-center gap-1.5 border">
          {modes.map((m) => (
            <button
              key={m.id}
              onClick={() => updateConfig({ mode: m.id })}
              className={cn(
                "flex-1 flex items-center justify-center gap-2 py-3 md:py-4 rounded-xl text-xs md:text-sm font-black transition-all duration-300",
                activeMode === m.id 
                  ? "bg-card text-primary shadow-sm shadow-black/5" 
                  : "text-muted-foreground hover:text-foreground"
              )}
            >
              <m.icon className={cn("size-4 md:size-5", activeMode === m.id ? "text-primary" : "opacity-60")} />
              <span className="uppercase tracking-widest">{m.label}</span>
            </button>
          ))}
        </div>
      </div>

      {/* 3. PERFORMANCE GRID */}
      <div className="grid grid-cols-1 lg:grid-cols-12 gap-6 md:gap-8">
        
        {/* Real-time Traffic Metrics */}
        <div className="lg:col-span-12 grid grid-cols-2 md:grid-cols-5 gap-4 md:gap-6 px-1">
          <StatCard title="实时下载" value={formatSpeed(stats.down).split(' ')[0]} unit={formatSpeed(stats.down).split(' ')[1]} icon={ArrowDown} color="blue" />
          <StatCard title="实时上传" value={formatSpeed(stats.up).split(' ')[0]} unit={formatSpeed(stats.up).split(' ')[1]} icon={ArrowUp} color="green" />
          <StatCard title="活跃连接" value={connections.length} unit="CONN" icon={Activity} />
          <StatCard title="混合协议端口" value={config.mixed_port} unit="PORT" icon={Network} />
          <StatCard title="内核状态" value={getCoreStateMeta(coreStatus?.state).short} unit={coreStatus?.pid ? `PID ${coreStatus.pid}` : 'CORE'} icon={Cpu} />
        </div>

        {/* System Intelligence & Network Profiling */}
        <div className="lg:col-span-8 px-1">
          <div className="bg-card border rounded-[2rem] p-8 md:p-12 flex flex-col md:flex-row items-center justify-between relative overflow-hidden group shadow-sm">
            <div className="relative z-10 text-left space-y-4 w-full">
               <div className="flex items-center gap-2 px-3 py-1 bg-primary/10 text-primary rounded-full w-fit text-[10px] font-black uppercase tracking-widest">
                  <Radio className="size-3 animate-pulse" />
                  Primary Ingress profiling
               </div>
               <div className="space-y-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <p className="text-xs font-bold text-muted-foreground uppercase tracking-wider">出口 IP 地理画像</p>
                    {egress.source && (
                      <span className="inline-flex items-center gap-1 text-[10px] font-bold text-muted-foreground">
                        <Radio className="size-3" />
                        数据源 {egress.source}
                      </span>
                    )}
                  </div>
                  <p className="text-3xl md:text-6xl font-mono font-black tracking-tighter">{egress.ip ?? (egressLoading ? '检测中' : '未连接')}</p>
               </div>
               <div className="flex flex-wrap gap-4 pt-2">
                 <div className="flex items-center gap-2 text-sm font-black uppercase tracking-tight text-foreground bg-muted px-4 py-2 rounded-xl">
                   <Network className="size-4 text-primary" />
                   {egress.provider ?? 'Unknown Provider'}
                 </div>
                 <div className="flex items-center gap-2 text-sm font-black uppercase tracking-tight text-foreground bg-muted px-4 py-2 rounded-xl">
                   <Globe2 className="size-4 text-primary" />
                   {egress.country ?? 'Unknown Region'}
                 </div>
               </div>
            </div>
            <Globe2 className="absolute -right-12 -bottom-12 size-48 md:size-80 text-primary/5 -rotate-12 transition-transform duration-1000 group-hover:rotate-12 group-hover:scale-110" />
          </div>
        </div>

        {/* Connection Quick View placeholder - can be expanded later */}
        <div className="lg:col-span-4 px-1 hidden lg:block">
           <div className="h-full bg-muted/40 border-2 border-dashed rounded-[2rem] flex flex-col items-center justify-center p-8 text-center space-y-4 opacity-80">
              <div className="size-16 bg-card rounded-3xl flex items-center justify-center shadow-inner">
                 <Activity className="size-8 text-muted-foreground" />
              </div>
              <div className="space-y-1">
                 <p className="text-sm font-black uppercase">Traffic Analyzer</p>
                 <p className="text-[10px] font-bold text-muted-foreground uppercase tracking-widest">Advanced visualization coming soon</p>
              </div>
           </div>
        </div>

      </div>
    </div>
  );
};
