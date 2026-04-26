import { useState, useEffect } from 'react';
import { 
  Settings as SettingsIcon, 
  Cpu, 
  Globe, 
  Network, 
  Monitor,
  Cloud,
  Save,
  RotateCcw,
  ChevronRight,
  Info,
  Link2,
  TerminalSquare,
  Server,
  ShieldAlert,
  Eye,
  EyeOff,
  Loader2,
  Database,
  Zap,
  Trash2,
  Search,
  HardDrive,
  CheckCircle2,
  Wand2,
  Smartphone,
  Layers,
  AlertCircle
} from 'lucide-react';
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { useToast } from './Toast';

interface Config {
  allow_lan: boolean;
  ipv6: boolean;
  log_level: string;
  mixed_port: number;
  external_controller: string;
  external_controller_enabled: boolean;
  secret: string;
  dns_enabled: boolean;
  dns_mode: string;
  store_selected: boolean;
  unified_delay: boolean;
  tcp_concurrent: boolean;
}

const BentoCard = ({ title, icon: Icon, children, className, description }: any) => (
  <div className={cn(
    "bg-card border rounded-[2.5rem] p-8 shadow-sm flex flex-col gap-6 text-left transition-all hover:shadow-md",
    className
  )}>
    <div className="flex items-center gap-3 border-b pb-4 text-left">
      <Icon className="size-5 text-primary" />
      <div className="text-left">
        <h3 className="text-lg font-black uppercase tracking-tighter text-foreground">{title}</h3>
        {description && <p className="text-[9px] font-bold text-muted-foreground uppercase tracking-widest">{description}</p>}
      </div>
    </div>
    <div className="flex-1 text-left">
      {children}
    </div>
  </div>
);

export const Settings = () => {
  const { toast } = useToast();
  const [isExpert, setIsExpert] = useState(false);
  const [config, setConfig] = useState<Config>({
    allow_lan: false,
    ipv6: true,
    log_level: 'info',
    mixed_port: 7890,
    external_controller: '127.0.0.1:9090',
    external_controller_enabled: true,
    secret: 'r-clash-secret-2024',
    dns_enabled: true,
    dns_mode: 'fake-ip',
    store_selected: true,
    unified_delay: true,
    tcp_concurrent: false
  });

  const [isSaving, setIsSaving] = useState(false);
  const [isFlushing, setIsFlushing] = useState(false);
  const [showSecret, setShowSecret] = useState(false);

  useEffect(() => { fetchConfig(); }, []);

  const fetchConfig = async () => {
    try {
      const res = await fetch('/api/configs');
      const data = await res.json();
      setConfig(prev => ({ ...prev, ...data }));
    } catch (e) {}
  };

  const saveConfig = async (updates: Partial<Config> = config) => {
    setIsSaving(true);
    try {
      const res = await fetch('/api/configs', {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(updates)
      });
      const data = await res.json();
      setConfig(data);
      toast('核心配置已应用', 'success');
    } catch (e) { toast('同步失败', 'error'); } finally { setIsSaving(false); }
  };

  const updateField = (field: keyof Config, value: any) => {
    const newConfig = { ...config, [field]: value };
    setConfig(newConfig);
    if (typeof value === 'boolean' || field === 'log_level' || field === 'dns_mode') {
      saveConfig({ [field]: value });
    }
  };

  // 检测专家模式修改导致的“套餐偏离”
  const isSpeedSurfingCustom = config.dns_mode === 'fake-ip' && (!config.tcp_concurrent || !config.unified_delay);
  const isCompatibleCustom = config.dns_mode === 'redir-host' && config.tcp_concurrent;

  const SettingRow = ({ label, description, children, icon: Icon, disabled }: any) => (
    <div className={cn("flex items-center justify-between py-5 border-b border-border/30 last:border-0 group transition-all duration-300", disabled && "opacity-30 grayscale pointer-events-none")}>
      <div className="flex items-center gap-4 text-left">
        {Icon && <div className="size-10 rounded-xl bg-muted flex items-center justify-center text-muted-foreground group-hover:bg-primary/10 group-hover:text-primary transition-colors shrink-0"><Icon className="size-5" /></div>}
        <div className="flex flex-col">
          <span className="text-sm font-black uppercase tracking-tight text-foreground">{label}</span>
          {description && <span className="text-[10px] font-bold text-muted-foreground uppercase tracking-widest leading-none mt-1">{description}</span>}
        </div>
      </div>
      <div className="shrink-0 ml-4">{children}</div>
    </div>
  );

  const MiniToggle = ({ active, onClick }: any) => (
    <button onClick={onClick} className={cn("relative w-11 h-6 rounded-full transition-all duration-500 p-1", active ? "bg-primary shadow-[0_0_10px_rgba(var(--primary),0.3)]" : "bg-muted-foreground/20")}>
      <div className={cn("size-4 rounded-full bg-white shadow-sm transition-all duration-500 transform", active ? "translate-x-5" : "translate-x-0")} />
    </button>
  );

  return (
    <div className="space-y-12 max-w-7xl mx-auto pb-12 animate-in fade-in duration-500 text-left px-1">
      
      {/* Header */}
      <div className="flex flex-col md:flex-row md:items-center justify-between gap-8">
        <div className="space-y-3">
          <div className="flex items-center gap-4">
            <div className="size-14 bg-primary/10 rounded-[1.5rem] flex items-center justify-center text-primary"><SettingsIcon className="size-7" /></div>
            <div>
              <h1 className="text-4xl font-black uppercase tracking-tighter">系统配置</h1>
              <div className="flex items-center gap-2 mt-1">
                 <span className={cn("size-2 rounded-full", config.external_controller_enabled ? "bg-green-500 animate-pulse" : "bg-slate-400")} />
                 <p className="text-[10px] font-black text-muted-foreground uppercase tracking-[0.2em]">Core v1.18.0 Meta • {isExpert ? "PRO MODE" : "STANDARD"}</p>
              </div>
            </div>
          </div>
        </div>
        
        <div className="bg-muted/50 p-1.5 rounded-2xl flex items-center gap-2 border">
           <button onClick={() => setIsExpert(false)} className={cn("px-6 py-2.5 rounded-xl text-xs font-black uppercase tracking-widest transition-all", !isExpert ? "bg-card text-primary shadow-sm" : "text-muted-foreground hover:text-foreground")}>基础模式</button>
           <button onClick={() => setIsExpert(true)} className={cn("px-6 py-2.5 rounded-xl text-xs font-black uppercase tracking-widest transition-all", isExpert ? "bg-card text-primary shadow-sm" : "text-muted-foreground hover:text-foreground")}>专家模式</button>
        </div>
      </div>

      {!isExpert ? (
        <div className="space-y-12 animate-in slide-in-from-bottom-4 duration-500">
           
           <div className="space-y-6">
              <div className="flex items-center gap-3 ml-2">
                 <Layers className="size-4 text-primary" />
                 <h2 className="text-xs font-black uppercase tracking-[0.3em] text-muted-foreground">核心预设 (基于 DNS 模式)</h2>
              </div>
              <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                 {/* 极速冲浪卡片 */}
                 <button 
                  onClick={() => { updateField('dns_mode', 'fake-ip'); updateField('tcp_concurrent', true); updateField('unified_delay', true); }}
                  className={cn(
                    "relative flex flex-col p-8 rounded-[2.5rem] border-2 text-left transition-all duration-500 group overflow-hidden",
                    config.dns_mode === 'fake-ip' ? "bg-card border-primary shadow-xl shadow-primary/5" : "bg-muted/30 border-transparent hover:border-primary/20 opacity-60"
                  )}
                 >
                    <div className={cn("size-12 rounded-xl flex items-center justify-center mb-4 transition-all", config.dns_mode === 'fake-ip' ? "bg-primary text-white" : "bg-card text-muted-foreground")}>
                      <Zap className="size-6" />
                    </div>
                    <div className="space-y-1 relative z-10">
                      <div className="flex items-center gap-2">
                        <h4 className="text-lg font-black uppercase">极速冲浪模式</h4>
                        {isSpeedSurfingCustom && <span className="bg-amber-500 text-black text-[8px] font-black px-1.5 py-0.5 rounded flex items-center gap-1 animate-pulse"><AlertCircle className="size-2" /> 专家修改过</span>}
                      </div>
                      <p className="text-[10px] font-bold text-muted-foreground uppercase leading-relaxed">
                        {isSpeedSurfingCustom ? "当前开启了 Fake-IP 但关闭了辅助加速，点击可恢复推荐值。" : "网页秒开，推荐 99% 的日常用户使用。"}
                      </p>
                    </div>
                    {config.dns_mode === 'fake-ip' && !isSpeedSurfingCustom && <CheckCircle2 className="absolute top-8 right-8 size-5 text-primary" />}
                 </button>

                 {/* 兼容模式卡片 */}
                 <button 
                  onClick={() => { updateField('dns_mode', 'redir-host'); updateField('tcp_concurrent', false); }}
                  className={cn(
                    "relative flex flex-col p-8 rounded-[2.5rem] border-2 text-left transition-all duration-500 group overflow-hidden",
                    config.dns_mode === 'redir-host' ? "bg-card border-primary shadow-xl shadow-primary/5" : "bg-muted/30 border-transparent hover:border-primary/20 opacity-60"
                  )}
                 >
                    <div className={cn("size-12 rounded-xl flex items-center justify-center mb-4 transition-all", config.dns_mode === 'redir-host' ? "bg-primary text-white" : "bg-card text-muted-foreground")}>
                      <ShieldAlert className="size-6" />
                    </div>
                    <div className="space-y-1 relative z-10">
                      <div className="flex items-center gap-2">
                        <h4 className="text-lg font-black uppercase">极致兼容模式</h4>
                        {isCompatibleCustom && <span className="bg-amber-500 text-black text-[8px] font-black px-1.5 py-0.5 rounded">自定义</span>}
                      </div>
                      <p className="text-[10px] font-bold text-muted-foreground uppercase leading-relaxed">显示真实 IP，解决特定软件、网游无法联网问题。</p>
                    </div>
                    {config.dns_mode === 'redir-host' && <CheckCircle2 className="absolute top-8 right-8 size-5 text-primary" />}
                 </button>
              </div>
           </div>

           <div className="space-y-6">
              <div className="flex items-center gap-3 ml-2">
                 <Wand2 className="size-4 text-primary" />
                 <h2 className="text-xs font-black uppercase tracking-[0.3em] text-muted-foreground">功能扩展</h2>
              </div>
              <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                 <div className="bg-card border rounded-[2.5rem] p-8 flex items-center justify-between group hover:border-primary/20 transition-all">
                    <div className="flex items-center gap-5">
                       <div className={cn("size-12 rounded-xl flex items-center justify-center", config.allow_lan ? "bg-green-500/10 text-green-500" : "bg-muted text-muted-foreground")}>
                          <Smartphone className="size-6" />
                       </div>
                       <div className="text-left">
                          <h4 className="text-base font-black uppercase">全屋设备共享</h4>
                          <p className="text-[10px] font-bold text-muted-foreground uppercase">允许手机、电视连接此代理</p>
                       </div>
                    </div>
                    <MiniToggle active={config.allow_lan} onClick={() => updateField('allow_lan', !config.allow_lan)} />
                 </div>

                 <div className="bg-card border rounded-[2.5rem] p-8 flex items-center justify-between group hover:border-primary/20 transition-all">
                    <div className="flex items-center gap-5">
                       <div className={cn("size-12 rounded-xl flex items-center justify-center", config.store_selected ? "bg-blue-500/10 text-blue-500" : "bg-muted text-muted-foreground")}>
                          <HardDrive className="size-6" />
                       </div>
                       <div className="text-left">
                          <h4 className="text-base font-black uppercase">节点选择记忆</h4>
                          <p className="text-[10px] font-bold text-muted-foreground uppercase">重启后自动恢复上次选中的节点</p>
                       </div>
                    </div>
                    <MiniToggle active={config.store_selected} onClick={() => updateField('store_selected', !config.store_selected)} />
                 </div>
              </div>
           </div>

           {/* 提示栏：检测到专家模式干扰 */}
           {(isSpeedSurfingCustom || isCompatibleCustom) && (
              <div className="p-5 rounded-2xl bg-amber-500/10 border border-amber-500/20 flex items-center gap-4 animate-in fade-in zoom-in duration-500">
                 <AlertCircle className="size-5 text-amber-600 shrink-0" />
                 <p className="text-xs font-bold text-amber-800 uppercase tracking-wide">
                    检测到您曾在“专家模式”下修改过底层参数。基础预设已进入“自定义”状态。点击上方卡片可恢复官方推荐值。
                 </p>
              </div>
           )}
        </div>
      ) : (
        /* 专家模式 (保持不变，已精简) */
        <div className="grid grid-cols-1 lg:grid-cols-12 gap-8 text-left animate-in fade-in slide-in-from-top-4 duration-500">
           <div className="lg:col-span-12 flex justify-between items-center bg-primary/5 p-6 rounded-[2rem] border border-primary/10">
              <div className="flex items-center gap-3">
                 <ShieldAlert className="size-5 text-primary" />
                 <p className="text-xs font-black uppercase text-primary tracking-widest">专家模式：所有更改将直接写入内核配置文件，请谨慎操作。</p>
              </div>
              <Button onClick={() => saveConfig()} disabled={isSaving} className="h-12 px-8 bg-zinc-900 text-white rounded-xl font-black uppercase tracking-widest shadow-xl">
                 {isSaving ? <Loader2 className="size-4 animate-spin" /> : <Save className="size-4 mr-2" />} 保存全部
              </Button>
           </div>
           
           <BentoCard title="运行参数" icon={Cpu} className="lg:col-span-4">
              <SettingRow label="允许局域网" icon={Link2}><MiniToggle active={config.allow_lan} onClick={() => updateField('allow_lan', !config.allow_lan)} /></SettingRow>
              <SettingRow label="IPv6 支持" icon={Globe}><MiniToggle active={config.ipv6} onClick={() => updateField('ipv6', !config.ipv6)} /></SettingRow>
              <SettingRow label="日志等级" icon={TerminalSquare}>
                 <select value={config.log_level} onChange={e => updateField('log_level', e.target.value)} className="bg-muted px-3 py-1 rounded-lg text-[10px] font-black uppercase outline-none">
                    <option value="info">Info</option><option value="warning">Warning</option><option value="error">Error</option>
                 </select>
              </SettingRow>
           </BentoCard>

           <BentoCard title="解析增强" icon={Search} className="lg:col-span-8">
              <div className="grid grid-cols-2 gap-x-12">
                 <SettingRow label="内核 DNS" icon={ShieldAlert}><MiniToggle active={config.dns_enabled} onClick={() => updateField('dns_enabled', !config.dns_enabled)} /></SettingRow>
                 <SettingRow label="DNS 模式" icon={Zap} disabled={!config.dns_enabled}>
                    <select value={config.dns_mode} onChange={e => updateField('dns_mode', e.target.value)} className="bg-muted px-3 py-1 rounded-lg text-[10px] font-black uppercase outline-none">
                       <option value="fake-ip">Fake-IP</option><option value="redir-host">Redir-Host</option>
                    </select>
                 </SettingRow>
                 <SettingRow label="TCP 并发" icon={Loader2}><MiniToggle active={config.tcp_concurrent} onClick={() => updateField('tcp_concurrent', !config.tcp_concurrent)} /></SettingRow>
                 <SettingRow label="体感延迟" icon={Zap}><MiniToggle active={config.unified_delay} onClick={() => updateField('unified_delay', !config.unified_delay)} /></SettingRow>
              </div>
           </BentoCard>

           <BentoCard title="网络与授权" icon={Server} className="lg:col-span-12">
              <div className="grid grid-cols-1 md:grid-cols-2 gap-x-16">
                 <SettingRow label="混合端口" icon={Network}><input type="number" value={config.mixed_port} onChange={e => updateField('mixed_port', parseInt(e.target.value))} className="w-24 h-10 bg-muted/50 rounded-xl px-4 text-right font-mono text-sm font-black text-primary outline-none" /></SettingRow>
                 <SettingRow label="节点记忆" icon={HardDrive}><MiniToggle active={config.store_selected} onClick={() => updateField('store_selected', !config.store_selected)} /></SettingRow>
              </div>
           </BentoCard>
        </div>
      )}
    </div>
  );
};
