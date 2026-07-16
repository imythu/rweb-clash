import { useState, useEffect, useCallback, useRef, type ReactNode } from 'react';
import { 
  Settings as SettingsIcon, 
  Cpu, 
  Globe, 
  Network, 
  Save,
  RotateCcw,
  Link2,
  TerminalSquare,
  Server,
  ShieldAlert,
  Eye,
  EyeOff,
  Loader2,
  Database,
  Zap,
  Search,
  HardDrive,
  CheckCircle2,
  Wand2,
  Smartphone,
  Layers,
  AlertCircle
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { useToast } from './toast-context';
import { api, type SystemConfig } from '@/lib/api';

interface BentoCardProps {
  title: string;
  icon: LucideIcon;
  children: ReactNode;
  className?: string;
  description?: string;
}

const BentoCard = ({ title, icon: Icon, children, className, description }: BentoCardProps) => (
  <div className={cn(
    "bg-card border rounded-[2.5rem] p-8 shadow-sm flex flex-col gap-6 text-left transition-all hover:shadow-md",
    className
  )}>
    <div className="flex items-center gap-3 border-b pb-4 text-left">
      <Icon className="size-5 text-primary" />
      <div className="text-left">
        <h3 className="text-lg font-black uppercase tracking-tighter text-foreground">{title}</h3>
        {description && <p className="text-[10px] font-bold text-muted-foreground uppercase tracking-wider">{description}</p>}
      </div>
    </div>
    <div className="flex-1 text-left">
      {children}
    </div>
  </div>
);

interface SettingRowProps {
  label: string;
  description?: string;
  children: ReactNode;
  icon?: LucideIcon;
  disabled?: boolean;
}

const SettingRow = ({ label, description, children, icon: Icon, disabled }: SettingRowProps) => (
  <div className={cn("flex items-center justify-between py-5 border-b border-border/30 last:border-0 group transition-all duration-300", disabled && "opacity-60 grayscale pointer-events-none")}>
    <div className="flex items-center gap-4 text-left">
      {Icon && <div className="size-10 rounded-xl bg-muted flex items-center justify-center text-muted-foreground group-hover:bg-primary/10 group-hover:text-primary transition-colors shrink-0"><Icon className="size-5" /></div>}
      <div className="flex flex-col">
        <span className="text-sm font-black uppercase tracking-tight text-foreground">{label}</span>
        {description && <span className="text-[10px] font-bold text-muted-foreground uppercase tracking-wider leading-none mt-1">{description}</span>}
      </div>
    </div>
    <div className="shrink-0 ml-4">{children}</div>
  </div>
);

interface MiniToggleProps {
  active: boolean;
  label: string;
  onClick: () => void;
}

const MiniToggle = ({ active, label, onClick }: MiniToggleProps) => (
  <button
    type="button"
    role="switch"
    aria-checked={active}
    aria-label={label}
    onClick={onClick}
    className={cn("relative w-11 h-6 rounded-full transition-all duration-500 p-1", active ? "bg-primary shadow-[0_0_10px_rgba(var(--primary),0.3)]" : "bg-muted-foreground/20")}
  >
    <span className={cn("block size-4 rounded-full bg-white shadow-sm transition-all duration-500 transform", active ? "translate-x-5" : "translate-x-0")} />
  </button>
);

export const Settings = () => {
  const { toast } = useToast();
  const [isExpert, setIsExpert] = useState(false);
  const [config, setConfig] = useState<SystemConfig | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [isSaving, setIsSaving] = useState(false);
  const [isFlushing, setIsFlushing] = useState(false);
  const [showSecret, setShowSecret] = useState(false);
  const loadRequestId = useRef(0);
  const saveInFlight = useRef(false);

  const fetchConfig = useCallback(async () => {
    const requestId = ++loadRequestId.current;
    setIsLoading(true);
    setLoadError(null);
    try {
      const data = await api.getConfig();
      if (loadRequestId.current === requestId) setConfig(data);
    } catch {
      if (loadRequestId.current === requestId) {
        setConfig(null);
        setLoadError('核心配置加载失败，请检查服务状态后重试。');
      }
    } finally {
      if (loadRequestId.current === requestId) setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    queueMicrotask(() => void fetchConfig());
    return () => { loadRequestId.current += 1; };
  }, [fetchConfig]);

  const saveConfig = async (updates: Partial<SystemConfig>, rollbackOnError = true) => {
    if (!config || saveInFlight.current) return;
    const previousConfig = config;
    const optimisticConfig = { ...config, ...updates };
    const updatedFields = Object.keys(updates) as Array<keyof SystemConfig>;
    saveInFlight.current = true;
    setIsSaving(true);
    setConfig(optimisticConfig);
    try {
      const data = await api.patchConfig(updates);
      setConfig(current => {
        if (!current) return data;
        const reconciled = { ...current };
        for (const field of updatedFields) {
          if (Object.is(current[field], optimisticConfig[field])) {
            Object.assign(reconciled, { [field]: data[field] });
          }
        }
        return reconciled;
      });
      toast('核心配置已应用', 'success');
    } catch {
      if (rollbackOnError) {
        setConfig(current => {
          if (!current) return previousConfig;
          const reconciled = { ...current };
          for (const field of updatedFields) {
            if (Object.is(current[field], optimisticConfig[field])) {
              Object.assign(reconciled, { [field]: previousConfig[field] });
            }
          }
          return reconciled;
        });
      }
      toast(rollbackOnError ? '同步失败，已恢复原配置' : '同步失败，修改已保留，可重新保存', 'error');
    } finally {
      saveInFlight.current = false;
      setIsSaving(false);
    }
  };

  const updateField = <K extends keyof SystemConfig>(field: K, value: SystemConfig[K]) => {
    if (typeof value === 'boolean' || field === 'log_level' || field === 'dns_mode') {
      void saveConfig({ [field]: value });
    } else {
      setConfig(current => current ? { ...current, [field]: value } : current);
    }
  };

  const flushDns = async () => {
    setIsFlushing(true);
    try {
      await api.flushDns();
      toast('DNS 缓存已清空', 'success');
    } catch {
      toast('DNS 缓存清空失败', 'error');
    } finally {
      setIsFlushing(false);
    }
  };

  if (isLoading && !config) {
    return <div className="flex items-center justify-center h-[60vh]"><Loader2 className="size-8 animate-spin text-primary" /></div>;
  }

  if (loadError || !config) {
    return (
      <div className="flex min-h-[60vh] items-center justify-center px-4">
        <div className="w-full max-w-md rounded-2xl border bg-card p-8 text-center shadow-sm">
          <AlertCircle className="mx-auto size-8 text-destructive" />
          <h2 className="mt-4 text-lg font-black">无法加载系统配置</h2>
          <p className="mt-2 text-sm font-bold text-muted-foreground">{loadError}</p>
          <Button type="button" onClick={() => void fetchConfig()} className="mt-6 rounded-xl" disabled={isLoading}>
            {isLoading ? <Loader2 className="animate-spin" /> : <RotateCcw />}
            重新加载
          </Button>
        </div>
      </div>
    );
  }

  // 检测专家模式修改导致的“套餐偏离”
  const isSpeedSurfingCustom = config.dns_mode === 'fake-ip' && (!config.tcp_concurrent || !config.unified_delay);
  const isCompatibleCustom = config.dns_mode === 'redir-host' && config.tcp_concurrent;

  return (
    <fieldset disabled={isSaving} aria-busy={isSaving} className="min-w-0 space-y-12 max-w-7xl mx-auto pb-12 animate-in fade-in duration-500 text-left px-1">
      
      {/* Header */}
      <div className="flex flex-col md:flex-row md:items-center justify-between gap-8">
        <div className="space-y-3">
          <div className="flex items-center gap-4">
            <div className="size-14 bg-primary/10 rounded-[1.5rem] flex items-center justify-center text-primary"><SettingsIcon className="size-7" /></div>
            <div>
              <h1 className="text-4xl font-black uppercase tracking-tighter">系统配置</h1>
              <div className="flex items-center gap-2 mt-1">
                 <span className={cn("size-2 rounded-full", config.external_controller_enabled ? "bg-green-500 animate-pulse" : "bg-slate-400")} />
                 <p className="text-[10px] font-black text-muted-foreground uppercase tracking-wider">Core v1.18.0 Meta • {isExpert ? "PRO MODE" : "STANDARD"}</p>
              </div>
            </div>
          </div>
        </div>
        
        <div className="bg-muted p-1.5 rounded-2xl flex items-center gap-2 border">
           <button type="button" onClick={() => setIsExpert(false)} className={cn("px-6 py-2.5 rounded-xl text-xs font-black uppercase tracking-widest transition-all", !isExpert ? "bg-card text-primary shadow-sm" : "text-muted-foreground hover:text-foreground")}>基础模式</button>
           <button type="button" onClick={() => setIsExpert(true)} className={cn("px-6 py-2.5 rounded-xl text-xs font-black uppercase tracking-widest transition-all", isExpert ? "bg-card text-primary shadow-sm" : "text-muted-foreground hover:text-foreground")}>专家模式</button>
        </div>
      </div>

      {!isExpert ? (
        <div className="space-y-12 animate-in slide-in-from-bottom-4 duration-500">
           
           <div className="space-y-6">
              <div className="flex items-center gap-3 ml-2">
                 <Layers className="size-4 text-primary" />
                 <h2 className="text-xs font-black uppercase tracking-wider text-muted-foreground">核心预设 (基于 DNS 模式)</h2>
              </div>
              <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                  {/* 极速冲浪卡片 */}
                  <button
                  type="button"
                  onClick={() => void saveConfig({ dns_mode: 'fake-ip', tcp_concurrent: true, unified_delay: true })}
                  className={cn(
                    "relative flex flex-col p-8 rounded-[2.5rem] border-2 text-left transition-all duration-500 group overflow-hidden",
                    config.dns_mode === 'fake-ip' ? "bg-card border-primary shadow-xl shadow-primary/5" : "bg-muted/40 border-transparent hover:border-primary/20"
                  )}
                 >
                    <div className={cn("size-12 rounded-xl flex items-center justify-center mb-4 transition-all", config.dns_mode === 'fake-ip' ? "bg-primary text-white" : "bg-card text-muted-foreground")}>
                      <Zap className="size-6" />
                    </div>
                    <div className="space-y-1 relative z-10">
                      <div className="flex items-center gap-2">
                        <h4 className="text-lg font-black uppercase">极速冲浪模式</h4>
                        {isSpeedSurfingCustom && <span className="bg-amber-500 text-black text-[10px] font-black px-1.5 py-0.5 rounded flex items-center gap-1 animate-pulse"><AlertCircle className="size-2" /> 专家修改过</span>}
                      </div>
                      <p className="text-xs font-bold text-muted-foreground uppercase leading-relaxed">
                        {isSpeedSurfingCustom ? "当前开启了 Fake-IP 但关闭了辅助加速，点击可恢复推荐值。" : "网页秒开，推荐 99% 的日常用户使用。"}
                      </p>
                    </div>
                    {config.dns_mode === 'fake-ip' && !isSpeedSurfingCustom && <CheckCircle2 className="absolute top-8 right-8 size-5 text-primary" />}
                 </button>

                  {/* 兼容模式卡片 */}
                  <button
                  type="button"
                  onClick={() => void saveConfig({ dns_mode: 'redir-host', tcp_concurrent: false })}
                  className={cn(
                    "relative flex flex-col p-8 rounded-[2.5rem] border-2 text-left transition-all duration-500 group overflow-hidden",
                    config.dns_mode === 'redir-host' ? "bg-card border-primary shadow-xl shadow-primary/5" : "bg-muted/40 border-transparent hover:border-primary/20"
                  )}
                 >
                    <div className={cn("size-12 rounded-xl flex items-center justify-center mb-4 transition-all", config.dns_mode === 'redir-host' ? "bg-primary text-white" : "bg-card text-muted-foreground")}>
                      <ShieldAlert className="size-6" />
                    </div>
                    <div className="space-y-1 relative z-10">
                      <div className="flex items-center gap-2">
                        <h4 className="text-lg font-black uppercase">极致兼容模式</h4>
                        {isCompatibleCustom && <span className="bg-amber-500 text-black text-[10px] font-black px-1.5 py-0.5 rounded">自定义</span>}
                      </div>
                      <p className="text-xs font-bold text-muted-foreground uppercase leading-relaxed">显示真实 IP，解决特定软件、网游无法联网问题。</p>
                    </div>
                    {config.dns_mode === 'redir-host' && <CheckCircle2 className="absolute top-8 right-8 size-5 text-primary" />}
                 </button>
              </div>
           </div>

           <div className="space-y-6">
              <div className="flex items-center gap-3 ml-2">
                 <Wand2 className="size-4 text-primary" />
                 <h2 className="text-xs font-black uppercase tracking-wider text-muted-foreground">功能扩展</h2>
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
                    <MiniToggle active={config.allow_lan} label="全屋设备共享" onClick={() => updateField('allow_lan', !config.allow_lan)} />
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
                    <MiniToggle active={config.store_selected} label="节点选择记忆" onClick={() => updateField('store_selected', !config.store_selected)} />
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
              <Button onClick={() => void saveConfig(config, false)} disabled={isSaving} className="h-12 px-8 bg-zinc-900 text-white rounded-xl font-black uppercase tracking-widest shadow-xl">
                 {isSaving ? <Loader2 className="size-4 animate-spin" /> : <Save className="size-4 mr-2" />} 保存全部
              </Button>
           </div>
           
           <BentoCard title="运行参数" icon={Cpu} className="lg:col-span-4">
              <SettingRow label="允许局域网" icon={Link2}><MiniToggle active={config.allow_lan} label="允许局域网" onClick={() => updateField('allow_lan', !config.allow_lan)} /></SettingRow>
              <SettingRow label="IPv6 支持" icon={Globe}><MiniToggle active={config.ipv6} label="IPv6 支持" onClick={() => updateField('ipv6', !config.ipv6)} /></SettingRow>
              <SettingRow label="日志等级" icon={TerminalSquare}>
                 <select value={config.log_level} onChange={e => updateField('log_level', e.target.value as SystemConfig['log_level'])} className="bg-muted px-3 py-1 rounded-lg text-[10px] font-black uppercase outline-none">
                    <option value="info">Info</option><option value="warning">Warning</option><option value="error">Error</option>
                 </select>
              </SettingRow>
           </BentoCard>

           <BentoCard title="解析增强" icon={Search} className="lg:col-span-8">
              <div className="grid grid-cols-2 gap-x-12">
                 <SettingRow label="内核 DNS" icon={ShieldAlert}><MiniToggle active={config.dns_enabled} label="内核 DNS" onClick={() => updateField('dns_enabled', !config.dns_enabled)} /></SettingRow>
                 <SettingRow label="DNS 模式" icon={Zap} disabled={!config.dns_enabled}>
                    <select value={config.dns_mode} onChange={e => updateField('dns_mode', e.target.value as SystemConfig['dns_mode'])} className="bg-muted px-3 py-1 rounded-lg text-[10px] font-black uppercase outline-none">
                       <option value="fake-ip">Fake-IP</option><option value="redir-host">Redir-Host</option>
                    </select>
                 </SettingRow>
                 <SettingRow label="DNS 缓存" icon={Database} disabled={!config.dns_enabled}>
                    <Button onClick={flushDns} disabled={isFlushing} variant="outline" className="h-10 rounded-xl px-4 text-[10px] font-black uppercase">
                       {isFlushing ? <Loader2 className="size-3.5 animate-spin mr-1.5" /> : <RotateCcw className="size-3.5 mr-1.5" />}
                       清空
                    </Button>
                 </SettingRow>
                 <SettingRow label="TCP 并发" icon={Loader2}><MiniToggle active={config.tcp_concurrent} label="TCP 并发" onClick={() => updateField('tcp_concurrent', !config.tcp_concurrent)} /></SettingRow>
                 <SettingRow label="体感延迟" icon={Zap}><MiniToggle active={config.unified_delay} label="体感延迟" onClick={() => updateField('unified_delay', !config.unified_delay)} /></SettingRow>
              </div>
           </BentoCard>

           <BentoCard title="网络与授权" icon={Server} className="lg:col-span-12">
              <div className="grid grid-cols-1 md:grid-cols-2 gap-x-16">
                 <SettingRow label="混合端口" icon={Network}><input type="number" value={config.mixed_port} onChange={e => { if (Number.isFinite(e.target.valueAsNumber)) updateField('mixed_port', e.target.valueAsNumber); }} className="w-24 h-10 bg-muted/50 rounded-xl px-4 text-right font-mono text-sm font-black text-primary outline-none" /></SettingRow>
                 <SettingRow label="外部控制" icon={Server}><MiniToggle active={config.external_controller_enabled} label="外部控制" onClick={() => updateField('external_controller_enabled', !config.external_controller_enabled)} /></SettingRow>
                 <SettingRow label="控制地址" icon={Link2}><input value={config.external_controller} onChange={e => updateField('external_controller', e.target.value)} className="w-44 h-10 bg-muted/50 rounded-xl px-4 text-right font-mono text-xs font-black text-primary outline-none" /></SettingRow>
                 <SettingRow label="API Secret" icon={ShieldAlert}>
                    <div className="flex items-center gap-2">
                      <input type={showSecret ? 'text' : 'password'} value={config.secret} onChange={e => updateField('secret', e.target.value)} className="w-44 h-10 bg-muted/50 rounded-xl px-4 text-right font-mono text-xs font-black text-primary outline-none" />
                       <Button type="button" variant="ghost" size="icon" aria-label={showSecret ? '隐藏 API Secret' : '显示 API Secret'} onClick={() => setShowSecret(!showSecret)} className="size-9 rounded-xl">
                        {showSecret ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
                      </Button>
                    </div>
                 </SettingRow>
                 <SettingRow label="自动启动内核" icon={Cpu}><MiniToggle active={config.auto_start} label="自动启动内核" onClick={() => updateField('auto_start', !config.auto_start)} /></SettingRow>
                 <SettingRow label="节点记忆" icon={HardDrive}><MiniToggle active={config.store_selected} label="节点记忆" onClick={() => updateField('store_selected', !config.store_selected)} /></SettingRow>
              </div>
           </BentoCard>
        </div>
      )}
    </fieldset>
  );
};
