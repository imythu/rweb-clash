import { useState, useEffect } from 'react';
import { 
  ArrowUp, 
  ArrowDown, 
  Activity, 
  Globe2, 
  Clock,
  Zap,
  Shield,
  Cpu,
  Compass,
  ZapOff,
  RefreshCcw,
  CheckCircle2
} from 'lucide-react';
import { cn } from "@/lib/utils";
import { useToast } from './Toast';

const StatCard = ({ title, value, unit, icon: Icon, color }: any) => (
  <div className="bg-card/50 backdrop-blur-md border rounded-2xl p-3 md:p-6 shadow-sm relative overflow-hidden">
    <div className="flex justify-between items-start relative z-10">
      <div className="min-w-0">
        <p className="text-[9px] md:text-sm font-black text-muted-foreground mb-0.5 uppercase tracking-tighter truncate">{title}</p>
        <div className="flex items-baseline gap-0.5">
          <span className="text-xl md:text-3xl font-black tracking-tighter">{value}</span>
          <span className="text-[8px] md:text-sm text-muted-foreground font-bold">{unit}</span>
        </div>
      </div>
      <Icon className={cn("size-4 md:size-6 opacity-20", color === 'blue' ? "text-blue-500" : "text-green-500")} />
    </div>
  </div>
);

const MasterSwitch = ({ icon: Icon, label, active, onClick }: any) => (
  <div 
    onClick={onClick}
    className="flex-1 group relative flex items-center justify-between p-3 md:p-6 rounded-[1.25rem] md:rounded-[2.5rem] transition-all duration-300 bg-card border hover:border-primary/30 cursor-pointer shadow-sm active:scale-[0.98]"
  >
    <div className="flex items-center gap-2 md:gap-4">
      <div className={cn(
        "size-10 md:size-14 rounded-xl md:rounded-[1.25rem] flex items-center justify-center transition-all duration-500",
        active ? "bg-primary/10 text-primary" : "bg-muted text-muted-foreground"
      )}>
        <Icon className="size-5 md:size-7" />
      </div>
      <div className="flex flex-col">
        <span className="text-[10px] md:text-lg font-black uppercase tracking-tight">{label}</span>
        <span className={cn("text-[7px] md:text-xs font-bold uppercase", active ? "text-green-500" : "text-muted-foreground/50")}>
          {active ? 'Running' : 'Stopped'}
        </span>
      </div>
    </div>

    {/* Physical Toggle Element */}
    <div className={cn(
      "w-10 md:w-16 h-5 md:h-8 rounded-full p-1 transition-all duration-500 relative",
      active ? "bg-green-500 shadow-[0_0_15px_rgba(34,197,94,0.3)]" : "bg-muted-foreground/20 shadow-inner"
    )}>
      <div className={cn(
        "size-3 md:size-6 bg-white rounded-full transition-all duration-500 shadow-lg",
        active ? "translate-x-5 md:translate-x-8" : "translate-x-0"
      )} />
    </div>
  </div>
);

export const Dashboard = () => {
  const { toast } = useToast();
  const [activeMode, setActiveMode] = useState('rule');
  const [isTesting, setIsTesting] = useState(false);
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
      toast('Success', 'success');
    } catch (e) {
      toast('Error', 'error');
    }
  };

  const handleTest = async () => {
    setIsTesting(true);
    try {
      const res = await fetch('/api/nodes/test', { method: 'POST', body: JSON.stringify({ name: 'HK 05' }) });
      const data = await res.json();
      toast(`${data.delay}ms`, 'success');
    } finally {
      setIsTesting(false);
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
    <div className="space-y-4 md:space-y-10 max-w-7xl mx-auto pb-12 animate-in fade-in duration-500">
      
      {/* 1. MASTER SWITCHES - CLEAN & INTUITIVE */}
      <div className="flex gap-2 md:gap-8 px-1">
        <MasterSwitch icon={Shield} label="系统代理" active={config.system_proxy} onClick={() => updateConfig({ system_proxy: !config.system_proxy })} />
        <MasterSwitch icon={Zap} label="TUN 模式" active={config.tun} onClick={() => updateConfig({ tun: !config.tun })} />
      </div>

      {/* 2. MODE SELECTOR (SEGMENTED CONTROL) */}
      <div className="px-1">
        <div className="bg-muted/50 p-1 rounded-xl md:rounded-2xl flex items-center gap-1 border border-muted/50">
          {modes.map((m) => (
            <button
              key={m.id}
              onClick={() => updateConfig({ mode: m.id })}
              className={cn(
                "flex-1 flex items-center justify-center gap-1.5 py-1.5 md:py-4 rounded-lg md:rounded-xl text-[10px] md:text-sm font-black transition-all duration-300",
                activeMode === m.id 
                  ? "bg-card text-primary shadow-sm shadow-black/5" 
                  : "text-muted-foreground hover:text-foreground"
              )}
            >
              <m.icon className={cn("size-3 md:size-4", activeMode === m.id ? "text-primary" : "opacity-40")} />
              {m.label}
            </button>
          ))}
        </div>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-12 gap-4 md:gap-10">
        <div className="lg:col-span-8 space-y-4 md:space-y-10">
          
          {/* 3. ACTIVE NODE */}
          <div className="bg-card border rounded-[1.25rem] md:rounded-[3rem] p-4 md:p-10 shadow-sm relative overflow-hidden group">
             <div className="relative z-10 flex flex-col md:flex-row justify-between items-start md:items-center gap-4">
                <div className="space-y-1 md:space-y-4 w-full text-left">
                  <div className="flex items-center gap-1.5 px-2 py-0.5 bg-green-500/10 text-green-500 rounded-md w-fit text-[8px] md:text-xs font-black uppercase">
                    <span className="size-1 bg-green-500 rounded-full animate-pulse shadow-[0_0_8px_rgba(34,197,94,0.5)]" />
                    CONNECTED
                  </div>
                  <h3 className="text-xl md:text-5xl font-black tracking-tighter truncate text-left">香港 05 (IEPL)</h3>
                  <div className="flex items-center gap-4 text-[9px] md:text-base text-muted-foreground font-bold text-left">
                    <span className={cn(isTesting ? "animate-pulse" : "text-green-500")}>延迟: {isTesting ? '...' : '34ms'}</span>
                    <div className="w-px h-3 md:h-6 bg-muted" />
                    <span className="text-foreground">协议: SS</span>
                  </div>
                </div>
                <button onClick={handleTest} disabled={isTesting} className="w-full md:w-auto px-8 md:px-12 py-3 md:py-6 bg-zinc-900 text-white hover:bg-black rounded-xl md:rounded-[1.5rem] font-black text-xs md:text-lg shadow-xl transition-all active:scale-95 flex items-center justify-center gap-3">
                  <RefreshCcw className={cn("size-4 md:size-6", isTesting && "animate-spin")} />
                  测速
                </button>
             </div>
          </div>

          {/* 4. TRAFFIC STATS */}
          <div className="grid grid-cols-2 md:grid-cols-4 gap-2 md:gap-6">
            <StatCard title="下载" value={formatSpeed(stats.down).split(' ')[0]} unit={formatSpeed(stats.down).split(' ')[1]} icon={ArrowDown} color="blue" />
            <StatCard title="上传" value={formatSpeed(stats.up).split(' ')[0]} unit={formatSpeed(stats.up).split(' ')[1]} icon={ArrowUp} color="green" />
            <StatCard title="连接" value={connections.length} unit="CONN" icon={Activity} />
            <StatCard title="负载" value="12" unit="%" icon={Cpu} />
          </div>

          {/* 5. IP INFO */}
          <div className="bg-card border rounded-2xl p-4 md:p-8 flex items-center justify-between relative overflow-hidden">
            <div className="relative z-10 text-left">
               <p className="text-[8px] md:text-xs font-black text-primary uppercase tracking-widest mb-1">出口 IP 画像</p>
               <p className="text-sm md:text-2xl font-mono font-black">203.0.113.1</p>
               <div className="flex gap-3 mt-1 text-[8px] md:text-sm font-bold text-muted-foreground">
                 <span>Google Cloud</span>
                 <span>🇭🇰 香港</span>
               </div>
            </div>
            <Globe2 className="absolute -right-6 -bottom-6 size-24 md:size-48 text-primary/5 -rotate-12" />
          </div>
        </div>
      </div>
    </div>
  );
};
