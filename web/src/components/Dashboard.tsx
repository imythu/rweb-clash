import { useState, useEffect } from 'react';
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
  RefreshCcw,
  Network,
  Radio
} from 'lucide-react';
import { cn } from "@/lib/utils";
import { useToast } from './Toast';

const StatCard = ({ title, value, unit, icon: Icon, color }: any) => (
  <div className="bg-card/50 backdrop-blur-md border rounded-2xl p-4 md:p-6 shadow-sm relative overflow-hidden group hover:border-primary/20 transition-all">
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

const MasterSwitch = ({ icon: Icon, label, active, onClick }: any) => (
  <div 
    onClick={onClick}
    className="flex-1 group relative flex items-center justify-between p-4 md:p-6 rounded-[1.5rem] md:rounded-[2.5rem] transition-all duration-300 bg-card border hover:border-primary/30 cursor-pointer shadow-sm active:scale-[0.98]"
  >
    <div className="flex items-center gap-3 md:gap-4 text-left">
      <div className={cn(
        "size-10 md:size-14 rounded-xl md:rounded-[1.25rem] flex items-center justify-center transition-all duration-500",
        active ? "bg-primary/10 text-primary" : "bg-muted text-muted-foreground"
      )}>
        <Icon className="size-5 md:size-7" />
      </div>
      <div className="flex flex-col">
        <span className="text-sm md:text-lg font-black uppercase tracking-tight">{label}</span>
        <span className={cn("text-[9px] md:text-xs font-bold uppercase", active ? "text-green-500" : "text-muted-foreground/50")}>
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

export const Dashboard = () => {
  const { toast } = useToast();
  const [activeMode, setActiveMode] = useState('rule');
  const [stats, setStats] = useState({ up: 0, down: 0 });
  const [connections, setConnections] = useState<any[]>([]);
  const [config, setConfig] = useState({ tun: true, system_proxy: true, global_kill: false });

  useEffect(() => {
    const fetchData = async () => {
      try {
        const [trafficRes, connRes, configRes] = await Promise.all([
          fetch('/api/traffic'), fetch('/api/connections'), fetch('/api/configs')
        ]);
        const traffic = await trafficRes.json();
        const conns = await connRes.json();
        const cfg = await configRes.json();
        setStats({ up: traffic.up, down: traffic.down });
        setConnections(conns);
        setActiveMode(cfg.mode || 'rule');
        setConfig(prev => ({ ...prev, ...cfg }));
      } catch (e) {
        console.error("Fetch error:", e);
      }
    };
    fetchData();
    const timer = setInterval(fetchData, 2000);
    return () => clearInterval(timer);
  }, []);

  const updateConfig = async (updates: any) => {
    try {
      await fetch('/api/configs', { method: 'PATCH', body: JSON.stringify(updates) });
      setConfig(prev => ({ ...prev, ...updates }));
      if (updates.mode) setActiveMode(updates.mode);
      toast('状态同步成功', 'success');
    } catch (e) {
      toast('同步失败', 'error');
    }
  };

  const formatSpeed = (bytes: number) => {
    if (bytes > 1024 * 1024) return (bytes / (1024 * 1024)).toFixed(1) + ' MB/s';
    return (bytes / 1024).toFixed(0) + ' KB/s';
  };

  const modes = [
    { id: 'rule', label: '规则分流', icon: Shield },
    { id: 'global', label: '全局代理', icon: Compass },
    { id: 'direct', label: '绕过直连', icon: ZapOff },
  ];

  return (
    <div className="space-y-6 md:space-y-12 max-w-7xl mx-auto pb-12 animate-in fade-in duration-500">
      
      {/* 1. MASTER CONTROL ROW */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4 md:gap-8 px-1">
        <MasterSwitch icon={Shield} label="系统代理" active={config.system_proxy} onClick={() => updateConfig({ system_proxy: !config.system_proxy })} />
        <MasterSwitch icon={Zap} label="TUN 模式" active={config.tun} onClick={() => updateConfig({ tun: !config.tun })} />
      </div>

      {/* 2. ENGINE SELECTION */}
      <div className="px-1">
        <div className="bg-muted/50 p-1.5 rounded-2xl flex items-center gap-1.5 border border-muted/50">
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
              <m.icon className={cn("size-4 md:size-5", activeMode === m.id ? "text-primary" : "opacity-30")} />
              <span className="uppercase tracking-widest">{m.label}</span>
            </button>
          ))}
        </div>
      </div>

      {/* 3. PERFORMANCE GRID */}
      <div className="grid grid-cols-1 lg:grid-cols-12 gap-6 md:gap-8">
        
        {/* Real-time Traffic Metrics */}
        <div className="lg:col-span-12 grid grid-cols-2 md:grid-cols-4 gap-4 md:gap-6 px-1">
          <StatCard title="实时下载" value={formatSpeed(stats.down).split(' ')[0]} unit={formatSpeed(stats.down).split(' ')[1]} icon={ArrowDown} color="blue" />
          <StatCard title="实时上传" value={formatSpeed(stats.up).split(' ')[0]} unit={formatSpeed(stats.up).split(' ')[1]} icon={ArrowUp} color="green" />
          <StatCard title="活跃连接" value={connections.length} unit="CONN" icon={Activity} />
          <StatCard title="核心负载" value="8.4" unit="%" icon={Cpu} />
        </div>

        {/* System Intelligence & Network Profiling */}
        <div className="lg:col-span-8 px-1">
          <div className="bg-card border rounded-[2rem] p-8 md:p-12 flex flex-col md:flex-row items-center justify-between relative overflow-hidden group">
            <div className="relative z-10 text-left space-y-4 w-full">
               <div className="flex items-center gap-2 px-3 py-1 bg-primary/10 text-primary rounded-full w-fit text-[10px] font-black uppercase tracking-widest">
                  <Radio className="size-3 animate-pulse" />
                  Primary Ingress profiling
               </div>
               <div className="space-y-1">
                  <p className="text-xs font-bold text-muted-foreground uppercase tracking-[0.2em]">出口 IP 地理画像</p>
                  <p className="text-3xl md:text-6xl font-mono font-black tracking-tighter">203.0.113.1</p>
               </div>
               <div className="flex flex-wrap gap-4 pt-2">
                 <div className="flex items-center gap-2 text-sm font-black uppercase tracking-tight text-foreground/80 bg-muted/50 px-4 py-2 rounded-xl">
                   <Network className="size-4 text-primary" />
                   Google Cloud Platform
                 </div>
                 <div className="flex items-center gap-2 text-sm font-black uppercase tracking-tight text-foreground/80 bg-muted/50 px-4 py-2 rounded-xl">
                   <span>🇭🇰</span>
                   Hong Kong SAR
                 </div>
               </div>
            </div>
            <Globe2 className="absolute -right-12 -bottom-12 size-48 md:size-80 text-primary/5 -rotate-12 transition-transform duration-1000 group-hover:rotate-12 group-hover:scale-110" />
          </div>
        </div>

        {/* Connection Quick View placeholder - can be expanded later */}
        <div className="lg:col-span-4 px-1 hidden lg:block">
           <div className="h-full bg-muted/30 border-2 border-dashed rounded-[2rem] flex flex-col items-center justify-center p-8 text-center space-y-4 opacity-50">
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
