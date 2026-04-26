import { useState, useEffect } from 'react';
import { 
  ArrowUp, 
  ArrowDown, 
  Activity, 
  Server, 
  Globe2, 
  Clock,
  ChevronRight,
  Zap,
  Shield,
  Trash2,
  Cpu,
  MousePointer2,
  Lock,
  Compass,
  ZapOff,
  RefreshCcw,
  CheckCircle2
} from 'lucide-react';
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

const Sparkline = ({ color }: { color: string }) => (
  <svg className="absolute bottom-0 left-0 w-full h-12 opacity-30 pointer-events-none" viewBox="0 0 100 30" preserveAspectRatio="none">
    <path 
      d="M0 25 Q 10 15, 20 20 T 40 10 T 60 18 T 80 5 T 100 15" 
      fill="none" 
      stroke={color} 
      strokeWidth="2"
      className="animate-pulse"
    />
  </svg>
);

const StatCard = ({ title, value, unit, icon: Icon, color }: any) => (
  <div className="bg-card border rounded-2xl p-6 shadow-sm hover:shadow-md transition-all group overflow-hidden relative">
    <div className="flex justify-between items-start relative z-10">
      <div>
        <p className="text-sm font-medium text-muted-foreground mb-1">{title}</p>
        <div className="flex items-baseline gap-1">
          <span className="text-3xl font-bold tracking-tight">{value}</span>
          <span className="text-sm text-muted-foreground font-medium">{unit}</span>
        </div>
      </div>
      <div className={cn(
        "size-12 rounded-xl flex items-center justify-center transition-all duration-300 group-hover:scale-110",
        color === 'blue' ? "bg-blue-500/10 text-blue-500" : 
        color === 'green' ? "bg-green-500/10 text-green-500" : "bg-muted text-muted-foreground"
      )}>
        <Icon className="size-6" />
      </div>
    </div>
    <Sparkline color={color === 'blue' ? "#3b82f6" : color === 'green' ? "#22c55e" : "#94a3b8"} />
  </div>
);

const ModeTile = ({ icon: Icon, label, desc, active, onClick }: any) => (
  <button 
    onClick={onClick}
    className={cn(
      "flex-1 p-5 rounded-2xl border transition-all duration-300 flex flex-col items-start gap-3 relative overflow-hidden group text-left",
      active 
        ? "bg-primary text-primary-foreground shadow-xl shadow-primary/20 border-primary ring-2 ring-primary/20" 
        : "bg-card hover:bg-muted border-transparent text-muted-foreground"
    )}
  >
    <div className={cn(
      "size-10 rounded-xl flex items-center justify-center transition-transform group-hover:scale-110",
      active ? "bg-white/20" : "bg-muted"
    )}>
      <Icon className="size-5" />
    </div>
    <div>
      <p className="font-bold text-sm uppercase tracking-wider">{label}</p>
      <p className={cn("text-[10px] opacity-70 leading-tight", active ? "text-white" : "text-muted-foreground")}>{desc}</p>
    </div>
    {active && <div className="absolute top-2 right-2"><CheckCircle2 className="size-4" /></div>}
    <div className={cn(
      "absolute -right-4 -bottom-4 size-16 rounded-full blur-2xl transition-opacity",
      active ? "bg-white/10 opacity-100" : "bg-primary/5 opacity-0 group-hover:opacity-100"
    )} />
  </button>
);

const QuickToggle = ({ icon: Icon, label, active, onClick }: any) => (
  <button 
    onClick={onClick}
    className={cn(
      "flex items-center justify-between p-3.5 rounded-xl border transition-all",
      active 
        ? "bg-primary/5 border-primary/50 text-primary shadow-sm" 
        : "bg-muted/30 border-transparent text-muted-foreground hover:bg-muted/50"
    )}
  >
    <div className="flex items-center gap-3">
      <div className={cn(
        "size-8 rounded-lg flex items-center justify-center transition-colors",
        active ? "bg-primary text-primary-foreground" : "bg-muted"
      )}>
        <Icon className="size-4" />
      </div>
      <span className="text-sm font-bold">{label}</span>
    </div>
    <div className={cn(
      "w-8 h-4 rounded-full relative transition-colors",
      active ? "bg-primary" : "bg-muted-foreground/30"
    )}>
      <div className={cn(
        "absolute top-0.5 size-3 bg-white rounded-full transition-all shadow-sm",
        active ? "right-0.5" : "left-0.5"
      )} />
    </div>
  </button>
);

import { useToast } from './Toast';

export const Dashboard = () => {
  const { toast } = useToast();
  const [activeMode, setActiveMode] = useState('rule');
  const [isTesting, setIsTesting] = useState(false);
  const [dnsCleaned, setDnsCleaned] = useState(false);
  const [stats, setStats] = useState({ up: 0, down: 0 });
  const [connections, setConnections] = useState<any[]>([]);
  const [config, setConfig] = useState({ tun: true, system_proxy: true, global_kill: false });

  // 模拟定时刷新流量和连接
  useEffect(() => {
    const fetchData = async () => {
      try {
        const [trafficRes, connRes, configRes] = await Promise.all([
          fetch('/api/traffic'),
          fetch('/api/connections'),
          fetch('/api/configs')
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
      await fetch('/api/configs', {
        method: 'PATCH',
        body: JSON.stringify(updates)
      });
      setConfig(prev => ({ ...prev, ...updates }));
      if (updates.mode) setActiveMode(updates.mode);
      toast(`配置已更新: ${Object.keys(updates).join(', ')}`, 'success');
    } catch (e) {
      toast('配置更新失败', 'error');
    }
  };

  const handleTest = async () => {
    setIsTesting(true);
    toast('正在测试节点延迟...', 'info');
    try {
      const res = await fetch('/api/nodes/test', {
        method: 'POST',
        body: JSON.stringify({ name: '香港 05 (IEPL)' })
      });
      const data = await res.json();
      toast(`测速完成: ${data.delay}ms`, 'success');
    } finally {
      setIsTesting(false);
    }
  };

  const handleCleanDns = async () => {
    setDnsCleaned(true);
    await fetch('/api/dns/flush', { method: 'POST' });
    toast('DNS 缓存已清理', 'success');
    setTimeout(() => setDnsCleaned(false), 2000);
  };

  const formatSpeed = (bytes: number) => {
    if (bytes > 1024 * 1024) return (bytes / (1024 * 1024)).toFixed(1) + ' MB/s';
    return (bytes / 1024).toFixed(0) + ' KB/s';
  };

  const commonApps: any = {
    'google.com': { icon: Globe2, color: 'text-blue-500' },
    'github.com': { icon: Activity, color: 'text-zinc-400' },
    'netflix.com': { icon: Lock, color: 'text-red-500' },
    'local': { icon: Server, color: 'text-green-500' },
  };

  return (
    <div className="space-y-8 max-w-7xl mx-auto pb-12 animate-in fade-in duration-500">
      {/* Mode Switcher - High Priority Tiles */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <ModeTile 
          icon={Shield} 
          label="Rule 规则模式" 
          desc="基于分流规则自动选择节点，兼顾速度与访问权"
          active={activeMode === 'rule'}
          onClick={() => updateConfig({ mode: 'rule' })}
        />
        <ModeTile 
          icon={Compass} 
          label="Global 全局模式" 
          desc="所有流量强行通过选中代理节点，用于特殊访问"
          active={activeMode === 'global'}
          onClick={() => updateConfig({ mode: 'global' })}
        />
        <ModeTile 
          icon={ZapOff} 
          label="Direct 直连模式" 
          desc="不经过任何代理，直接使用本地宽带进行访问"
          active={activeMode === 'direct'}
          onClick={() => updateConfig({ mode: 'direct' })}
        />
      </div>

      {/* Real-time Stats Grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6">
        <StatCard title="实时下载" value={formatSpeed(stats.down).split(' ')[0]} unit={formatSpeed(stats.down).split(' ')[1]} icon={ArrowDown} color="blue" />
        <StatCard title="实时上传" value={formatSpeed(stats.up).split(' ')[0]} unit={formatSpeed(stats.up).split(' ')[1]} icon={ArrowUp} color="green" />
        <StatCard title="活动连接" value={connections.length} unit="Pipes" icon={Activity} />
        <StatCard title="核心负载" value="12" unit="%" icon={Cpu} />
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-12 gap-8">
        <div className="lg:col-span-8 space-y-8">
          {/* Active Node Info */}
          <div className="bg-card border rounded-3xl p-8 shadow-sm relative overflow-hidden group">
             <div className="relative z-10 flex flex-col md:flex-row justify-between items-start md:items-center gap-6">
                <div className="space-y-3">
                  <div className="flex items-center gap-2 px-3 py-1 bg-green-500/10 text-green-500 rounded-full w-fit text-[10px] font-black uppercase tracking-[0.2em]">
                    <span className="size-2 bg-green-500 rounded-full animate-pulse" />
                    已连接 · 链路健康
                  </div>
                  <h3 className="text-4xl font-black tracking-tighter">香港 05 (IEPL)</h3>
                  <div className="flex items-center gap-6 text-sm text-muted-foreground">
                    <div className="flex flex-col">
                      <span className="text-[10px] uppercase font-bold tracking-widest opacity-50">实时延迟</span>
                      <span className={cn(
                        "font-mono font-black text-lg transition-all",
                        isTesting ? "animate-pulse text-primary" : "text-green-500"
                      )}>
                        {isTesting ? 'Testing...' : '34ms'}
                      </span>
                    </div>
                    <div className="w-px h-8 bg-muted" />
                    <div className="flex flex-col">
                      <span className="text-[10px] uppercase font-bold tracking-widest opacity-50">运行协议</span>
                      <span className="font-bold text-foreground">Shadowsocks</span>
                    </div>
                  </div>
                </div>
                <div className="flex gap-3 shrink-0">
                  <button 
                    onClick={handleTest}
                    disabled={isTesting}
                    className="px-6 py-4 bg-primary text-primary-foreground rounded-2xl font-black hover:shadow-xl hover:shadow-primary/30 transition-all active:scale-95 flex items-center gap-2 disabled:opacity-50"
                  >
                    <RefreshCcw className={cn("size-4", isTesting && "animate-spin")} />
                    {isTesting ? '正在采集' : '测速'}
                  </button>
                  <button 
                    onClick={() => toast('节点选择器正在开发中...', 'info')}
                    className="px-6 py-4 bg-muted hover:bg-muted/80 rounded-2xl font-black transition-all flex items-center gap-2"
                  >
                    更换节点
                  </button>
                </div>
             </div>
             <div className="absolute right-0 top-0 w-1/3 h-full bg-gradient-to-l from-primary/5 to-transparent -z-0" />
          </div>

          {/* IP Insights */}
          <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
            <div className="bg-card border rounded-2xl p-6 space-y-6 relative overflow-hidden">
               <h4 className="font-bold flex items-center gap-2 text-primary relative z-10">
                 <Globe2 className="size-4" /> 出口 IP 画像
               </h4>
               <div className="space-y-4 relative z-10">
                 <div className="flex flex-col">
                    <span className="text-[10px] uppercase font-bold text-muted-foreground">公网地址</span>
                    <span className="text-xl font-mono font-black text-foreground">203.0.113.1</span>
                 </div>
                 <div className="grid grid-cols-2 gap-4">
                    <div className="flex flex-col">
                      <span className="text-[10px] uppercase font-bold text-muted-foreground">ISP</span>
                      <span className="text-sm font-bold">Google Cloud</span>
                    </div>
                    <div className="flex flex-col">
                      <span className="text-[10px] uppercase font-bold text-muted-foreground">地区</span>
                      <span className="text-sm font-bold flex items-center gap-1">🇭🇰 香港</span>
                    </div>
                 </div>
                 <div className="flex gap-2 pt-2">
                    <span className="px-3 py-1 bg-green-500/10 text-green-600 text-[10px] font-black rounded-full border border-green-500/20">Netflix</span>
                    <span className="px-3 py-1 bg-green-500/10 text-green-600 text-[10px] font-black rounded-full border border-green-500/20">YouTube</span>
                    <span className="px-3 py-1 bg-red-500/10 text-red-600 text-[10px] font-black rounded-full border border-red-500/20">ChatGPT</span>
                 </div>
               </div>
               <Globe2 className="absolute -right-12 -bottom-12 size-48 text-primary/5 -rotate-12" />
            </div>

            <div className="bg-card border rounded-2xl p-6 space-y-4 flex flex-col justify-between">
               <h4 className="font-bold flex items-center gap-2 uppercase tracking-widest text-xs">
                 <Clock className="size-4 text-primary" /> 常用节点
               </h4>
               <div className="space-y-4">
                 {[1, 2, 3].map(i => (
                   <div 
                    key={i} 
                    onClick={() => toast(`已成功切换到 常用节点 0${i}`, 'success')}
                    className="flex items-center justify-between group cursor-pointer hover:bg-muted/50 p-1 -mx-1 rounded-lg transition-colors"
                   >
                      <div className="flex items-center gap-3">
                        <div className="size-10 rounded-xl bg-muted flex items-center justify-center text-xs font-black group-hover:bg-primary group-hover:text-primary-foreground transition-all">HK</div>
                        <div className="flex flex-col">
                          <span className="text-sm font-bold">香港 0{i} (IEPL)</span>
                          <span className="text-[10px] text-muted-foreground">Shadowsocks · 1.5x</span>
                        </div>
                      </div>
                      <span className="text-sm font-mono text-green-500 font-black">2{i}ms</span>
                   </div>
                 ))}
               </div>
            </div>
          </div>

          {/* Mini Connection Feed with Smart Icons */}
          <div className="bg-card border rounded-3xl overflow-hidden shadow-sm">
            <div className="p-5 border-b bg-muted/20 flex justify-between items-center">
              <h4 className="text-sm font-black flex items-center gap-2 uppercase tracking-widest">
                <MousePointer2 className="size-4 text-primary" /> 实时流量轨迹
              </h4>
            </div>
            <div className="divide-y">
              {connections.map((conn) => {
                const appInfo = commonApps[conn.domain] || { icon: Globe2, color: 'text-muted-foreground' };
                const Icon = appInfo.icon;
                return (
                  <div key={conn.id} className="px-6 py-4 flex items-center justify-between hover:bg-muted/30 transition-colors">
                    <div className="flex items-center gap-4 min-w-0">
                      <div className={cn("size-10 rounded-2xl bg-muted flex items-center justify-center shrink-0", appInfo.color)}>
                        <Icon className="size-5" />
                      </div>
                      <div className="flex flex-col min-w-0">
                        <span className="text-sm font-black font-mono truncate">{conn.domain}</span>
                        <div className="flex items-center gap-2">
                          <span className="text-[10px] font-bold text-muted-foreground uppercase">{conn.rule}</span>
                          <div className="size-1 rounded-full bg-muted-foreground/30" />
                          <span className="text-[10px] font-black text-primary uppercase">{conn.policy}</span>
                        </div>
                      </div>
                    </div>
                    <span className="text-xs font-mono font-black text-muted-foreground bg-muted/50 px-2 py-1 rounded-lg">{conn.speed}</span>
                  </div>
                );
              })}
            </div>
          </div>
        </div>

        {/* Sidebar Controls */}
        <div className="lg:col-span-4 space-y-6">
          <div className="bg-card border rounded-3xl p-6 space-y-6 shadow-sm">
            <h4 className="font-bold flex items-center gap-2">
              <Settings2 className="size-4" /> 核心快速控制
            </h4>
            <div className="grid grid-cols-1 gap-3">
              <QuickToggle 
                icon={Shield} 
                label="系统代理" 
                active={config.system_proxy} 
                onClick={() => updateConfig({ system_proxy: !config.system_proxy })}
              />
              <QuickToggle 
                icon={Zap} 
                label="TUN 模式" 
                active={config.tun} 
                onClick={() => updateConfig({ tun: !config.tun })}
              />
              <QuickToggle 
                icon={Globe2} 
                label="全局断网" 
                active={config.global_kill} 
                onClick={() => updateConfig({ global_kill: !config.global_kill })}
              />
            </div>
            <div className="pt-4 border-t border-dashed">
              <Button 
                variant="outline" 
                onClick={handleCleanDns}
                className={cn(
                  "w-full justify-between gap-2 h-14 rounded-2xl group transition-all relative overflow-hidden",
                  dnsCleaned && "border-green-500 text-green-500 bg-green-500/5"
                )}
              >
                <div className="flex items-center gap-3 relative z-10">
                  {dnsCleaned ? <CheckCircle2 className="size-5" /> : <Trash2 className="size-5 text-destructive group-hover:animate-bounce" />}
                  <span className="font-black uppercase tracking-tight">{dnsCleaned ? '清理成功' : '清理 DNS 缓存'}</span>
                </div>
                {!dnsCleaned && <span className="text-[10px] bg-muted px-2 py-1 rounded-lg font-bold text-muted-foreground relative z-10">140 条</span>}
              </Button>
            </div>
          </div>

          <div className="bg-card border rounded-3xl p-6 space-y-6 shadow-sm">
            <h4 className="font-bold uppercase tracking-widest text-xs flex items-center gap-2">
               <Activity className="size-4 text-primary" /> 内核运行状态
            </h4>
            <div className="space-y-5">
              <div className="flex flex-col gap-1">
                <span className="text-[10px] uppercase font-bold text-muted-foreground">内核版本</span>
                <span className="text-sm font-mono font-black">Mihomo v1.18.0</span>
              </div>
              <div className="flex items-center justify-between">
                <div className="flex flex-col gap-1">
                  <span className="text-[10px] uppercase font-bold text-muted-foreground">运行时长</span>
                  <span className="text-sm font-mono font-bold">12h 45m 22s</span>
                </div>
                <div className="size-10 rounded-full bg-green-500/10 flex items-center justify-center">
                  <div className="size-2 rounded-full bg-green-500 animate-ping" />
                </div>
              </div>
            </div>
          </div>

          <div className="bg-primary rounded-[2rem] p-8 text-primary-foreground relative overflow-hidden shadow-2xl shadow-primary/40 group cursor-pointer">
             <div className="relative z-10">
                <h4 className="font-black text-xl mb-2">掌握高级规则</h4>
                <p className="text-sm opacity-80 leading-relaxed font-medium">
                  前往路由管理面板，配置自动识别规则集。
                </p>
                <button className="mt-6 size-10 rounded-xl bg-white/20 flex items-center justify-center group-hover:translate-x-2 transition-transform">
                  <ChevronRight className="size-5" />
                </button>
             </div>
             <Zap className="absolute -right-8 -bottom-8 size-48 opacity-10 rotate-12 transition-transform group-hover:scale-125" />
          </div>
        </div>
      </div>
    </div>
  );
};

const Settings2 = ({ className }: any) => (
  <svg className={className} xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M20 7h-9"/><path d="M14 17H5"/><circle cx="17" cy="17" r="3"/><circle cx="7" cy="7" r="3"/></svg>
);
