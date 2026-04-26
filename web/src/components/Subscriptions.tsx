import { useState, useEffect } from 'react';
import { 
  Plus, 
  Trash2, 
  Shield, 
  X,
  Loader2,
  CheckCircle2,
  ChevronDown,
  Activity,
  Layers,
  Info,
  ZapOff,
  ShieldCheck,
  ShieldAlert,
  Link,
  ChevronRight,
  Filter
} from 'lucide-react';
import { Button } from "@/components/ui/button";
import { useToast } from './Toast';
import { cn } from '@/lib/utils';

const SubscriptionCard = ({ sub, onEdit, onDelete }: any) => {
  const { toast } = useToast();
  const copyUrl = () => { navigator.clipboard.writeText(sub.url); toast('地址已复制', 'success'); };
  const formatBytes = (bytes: number) => {
    if (!bytes) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
  };
  const trafficPercent = Math.min(100, (sub.traffic.used / sub.traffic.total) * 100);

  // Status mapping for visual cues
  const statusConfig: any = {
    online: { color: 'bg-emerald-500', text: '在线', icon: CheckCircle2 },
    offline: { color: 'bg-rose-500', text: '失效', icon: ZapOff },
    syncing: { color: 'bg-blue-500', text: '同步中', icon: Loader2 }
  };
  const status = sub.status || 'online';
  const config = statusConfig[status] || statusConfig.online;

  return (
    <div className={cn(
      "relative p-[2px] rounded-[2rem] overflow-hidden group transition-all duration-500 hover:shadow-2xl hover:shadow-primary/10",
      status === 'syncing' ? "animate-border-flow" : "bg-transparent"
    )}>
      <div className="bg-card rounded-[1.95rem] overflow-hidden h-full flex flex-col border border-muted shadow-sm">
        {/* Bento Header */}
        <div className="p-3 md:p-5 pb-2 space-y-3 md:space-y-4">
          <div className="flex justify-between items-start gap-2">
            <div className="flex gap-3 md:gap-4 min-w-0 flex-1">
              <div className={cn(
                "size-10 md:size-14 rounded-2xl flex items-center justify-center font-black text-lg md:text-xl shadow-lg relative shrink-0 transition-transform group-hover:scale-105",
                sub.inheritGlobal ? "bg-primary text-primary-foreground" : "bg-zinc-800 text-white"
              )}>
                {sub.name?.charAt(0) || 'S'}
                <div className={cn(
                  "absolute -top-1 -right-1 size-4 md:size-5 rounded-full border-2 border-card flex items-center justify-center shadow-sm",
                  sub.inheritGlobal ? "bg-blue-500 text-white" : "bg-zinc-400 text-white"
                )}>
                  {sub.inheritGlobal ? <ShieldCheck className="size-2.5 md:size-3" /> : <ShieldAlert className="size-2.5 md:size-3" />}
                </div>
              </div>
              <div className="min-w-0 pt-0.5">
                <h4 className="font-black text-sm md:text-lg tracking-tight uppercase truncate">{sub.name || '未命名资源'}</h4>
                <div className="flex items-center gap-1.5 mt-0.5 opacity-40 hover:opacity-100 transition-opacity cursor-pointer group/url" onClick={copyUrl}>
                  <Link className="size-2.5 shrink-0" />
                  <p className="text-[8px] md:text-[9px] font-mono truncate max-w-[60px] md:max-w-[120px]">{sub.url}</p>
                </div>
              </div>
            </div>
            <button onClick={() => onDelete(sub.id)} className="size-7 md:size-8 rounded-xl bg-destructive/5 text-destructive flex items-center justify-center opacity-0 group-hover:opacity-100 hover:bg-rose-500 hover:text-white transition-all shrink-0"><Trash2 className="size-3.5 md:size-4" /></button>
          </div>
        </div>

        {/* Bento Grid Content */}
        <div className="flex-1 grid grid-cols-2 gap-1.5 md:gap-2 p-2 md:p-4 pt-0">
          {/* Traffic Block */}
          <div className="col-span-2 bg-muted/20 rounded-xl md:rounded-2xl p-3 md:p-4 border border-transparent hover:border-primary/10 transition-all">
            <div className="flex flex-col sm:flex-row justify-between items-start sm:items-end gap-1 mb-2">
              <span className="text-[7px] md:text-[8px] font-black text-muted-foreground uppercase tracking-widest">已用流量</span>
              <span className="text-[9px] md:text-[10px] font-black">{formatBytes(sub.traffic.used)} <span className="opacity-20">/ {formatBytes(sub.traffic.total)}</span></span>
            </div>
            <div className="h-1.5 md:h-2 w-full bg-muted rounded-full overflow-hidden shadow-inner border border-background">
              <div className={cn("h-full transition-all duration-1000 ease-out", trafficPercent > 90 ? "bg-rose-500" : "bg-primary")} style={{ width: `${trafficPercent}%` }} />
            </div>
          </div>

          {/* Expiry Block */}
          <div className="bg-muted/10 rounded-xl md:rounded-2xl p-2 md:p-3 flex flex-col justify-between hover:bg-muted/20 transition-all border border-transparent">
             <span className="text-[7px] font-black text-muted-foreground uppercase opacity-60">服务到期</span>
             <p className="text-[9px] md:text-[10px] font-black mt-0.5">{sub.expiry}</p>
          </div>

          {/* Status Block */}
          <div className="bg-muted/10 rounded-xl md:rounded-2xl p-2 md:p-3 flex flex-col justify-between hover:bg-muted/20 transition-all border border-transparent">
             <span className="text-[7px] font-black text-muted-foreground uppercase opacity-60">连接状态</span>
             <div className="flex items-center gap-1.5 mt-0.5">
                <div className={cn("size-1.5 md:size-2 rounded-full", config.color, status === 'syncing' && "animate-pulse")} />
                <span className={cn("text-[8px] md:text-[9px] font-black uppercase", status === 'offline' ? "text-rose-600" : "text-emerald-600")}>{config.text}</span>
             </div>
          </div>
        </div>

        {/* Bento Footer */}
        <div className="px-3 md:px-4 py-2.5 md:py-3 bg-muted/5 border-t border-dashed flex items-center justify-between">
           <div className="flex items-baseline gap-1 min-w-0">
              <p className="text-xs md:text-sm font-black tracking-tighter truncate">{sub.nodes}</p>
              <span className="text-[7px] md:text-[8px] font-bold text-primary/40 uppercase hidden xs:inline">Nodes</span>
           </div>
           <Button variant="outline" size="sm" onClick={() => onEdit(sub)} className="h-7 md:h-8 rounded-lg md:rounded-xl text-[8px] md:text-[9px] font-black uppercase border-2 px-2.5 md:px-4 shadow-sm hover:bg-primary hover:text-white hover:border-primary transition-all shrink-0">配置</Button>
        </div>
      </div>

      {/* Sync Failure Snapshot Overlay (Only for Offline) */}
      {status === 'offline' && sub.lastError && (
        <div className="absolute top-12 left-6 right-6 bg-rose-500 text-white p-2 rounded-lg text-[8px] font-bold shadow-xl animate-in zoom-in-95 pointer-events-none z-10 border border-white/20">
           ERROR: {sub.lastError}
        </div>
      )}
    </div>
  );
};

export const Subscriptions = () => {
  const { toast } = useToast();
  const [subs, setSubs] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const [editingSub, setEditingSub] = useState<any>(null);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [isGlobalDrawerOpen, setIsGlobalDrawerOpen] = useState(false);

  const [globalRules] = useState([
    { id: '1', action: 'discard', pattern: '.*(官网|剩余|到期).*', type: 'regex' },
    { id: '2', action: 'discard', pattern: '广告', type: 'contains' }
  ]);

  useEffect(() => { fetchSubs(); }, []);
  useEffect(() => { if (editingSub) setShowAdvanced(editingSub.rules?.length > 0); }, [editingSub]);

  const fetchSubs = async () => {
    const res = await fetch('/api/subscriptions');
    const data = await res.json();
    setSubs(data);
    setLoading(false);
  };

  const handleUpdateSub = async () => {
    // 1. Validate URL
    if (!editingSub.url.trim()) {
      toast('节点接口地址不能为空', 'error');
      return;
    }
    try {
      new URL(editingSub.url);
    } catch (_) {
      toast('节点接口地址格式不正确，请包含 http/https', 'error');
      return;
    }

    // 2. Validate Rules
    if (editingSub.rules && editingSub.rules.length > 0) {
      for (let i = 0; i < editingSub.rules.length; i++) {
        const rule = editingSub.rules[i];
        if (!rule.pattern.trim()) {
          toast(`规则 #${i + 1} 的关键字/匹配模式不能为空`, 'error');
          return;
        }
        if (rule.type === 'regex') {
          try {
            new RegExp(rule.pattern);
          } catch (e) {
            toast(`规则 #${i + 1} 的正则表达式不合法`, 'error');
            return;
          }
        }
      }
    }

    const method = editingSub.id ? 'PATCH' : 'POST';
    await fetch(editingSub.id ? `/api/subscriptions/${editingSub.id}` : '/api/subscriptions', { method, body: JSON.stringify(editingSub) });
    toast('配置已同步', 'success');
    setEditingSub(null);
    fetchSubs();
  };

  const handleDelete = async (id: string) => {
    if (confirm('确认移除该资源？')) {
      await fetch(`/api/subscriptions/${id}`, { method: 'DELETE' });
      toast('资源已移除', 'success');
      fetchSubs();
    }
  };

  const intervals = [
    { label: '1H', value: 60 }, { label: '6H', value: 360 }, { label: '12H', value: 720 }, { label: '24H', value: 1440 }, { label: 'NEVER', value: 0 },
  ];

  const [lastAddedIndex, setLastAddedIndex] = useState<number | null>(null);

  const handleAddRule = () => {
    const newRule = { pattern: '', action: 'keep', type: 'contains' };
    const newRules = [...editingSub.rules, newRule];
    setEditingSub({ ...editingSub, rules: newRules });
    setLastAddedIndex(newRules.length - 1);
  };

  const handleDeleteRule = (index: number) => {
    const newRules = [...editingSub.rules];
    newRules.splice(index, 1);
    setEditingSub({ ...editingSub, rules: newRules });
    setLastAddedIndex(null);
  };

  const handleUpdateRule = (index: number, updates: any) => {
    const newRules = [...editingSub.rules];
    newRules[index] = { ...newRules[index], ...updates };
    setEditingSub({ ...editingSub, rules: newRules });
  };

  const ruleTypes = [
    { value: 'contains', label: '包含' },
    { value: 'not_contains', label: '不包含' },
    { value: 'regex', label: '正则匹配' }
  ];

  const handleCycleRuleType = (index: number) => {
    const currentType = editingSub.rules[index].type || 'contains';
    const currentIndex = ruleTypes.findIndex(t => t.value === currentType);
    const nextIndex = (currentIndex + 1) % ruleTypes.length;
    handleUpdateRule(index, { type: ruleTypes[nextIndex].value });
  };

  if (loading) return <div className="flex items-center justify-center h-[60vh]"><Loader2 className="animate-spin text-primary size-8" /></div>;

  return (
    <div className="space-y-6 md:space-y-10 max-w-7xl mx-auto pb-20 animate-in fade-in duration-500 text-left">
      {/* Header */}
      <div className="flex justify-between items-center text-left px-2">
        <div>
          <h2 className="text-2xl md:text-3xl font-black uppercase tracking-tight text-left">订阅资源池</h2>
          <div className="flex items-center gap-2 mt-1 hidden sm:flex">
             <Info className="size-3 text-muted-foreground" />
             <p className="text-[9px] md:text-[10px] font-bold text-muted-foreground uppercase tracking-widest opacity-60">Smart Merge & Selection</p>
          </div>
        </div>
        <Button onClick={() => setEditingSub({ name: '', url: '', rules: [], traffic: {used:0, total: 100*1024**3}, interval: 360, inheritGlobal: true })} className="rounded-xl md:rounded-2xl gap-2 shadow-xl shadow-primary/20 font-black text-[9px] md:text-[10px] h-10 md:h-12 px-6 md:px-10 uppercase transition-all hover:scale-105 active:scale-95">
          <Plus className="size-4" /> 导入新资源
        </Button>
      </div>

      {/* GLOBAL RULES BANNER - REFACTORED FOR SPACE */}
      <div 
        onClick={() => setIsGlobalDrawerOpen(true)}
        className="bg-primary/[0.03] border-2 border-primary/10 rounded-[1.25rem] md:rounded-[1.75rem] p-3 md:p-4 mx-2 flex items-center justify-between hover:bg-primary/[0.06] transition-all cursor-pointer group relative overflow-hidden"
      >
         <div className="flex items-center gap-3 md:gap-5 relative z-10 text-left min-w-0 flex-1 mr-4">
           <div className="flex items-center gap-3 shrink-0">
             <div className="size-9 md:size-11 rounded-xl bg-primary text-white flex items-center justify-center shadow-lg shadow-primary/20 shrink-0"><Shield className="size-4 md:size-5" /></div>
             <h3 className="text-xs md:text-sm font-black uppercase tracking-widest hidden xs:block">通用精选准则</h3>
           </div>
           <div className="h-6 w-px bg-primary/10 hidden md:block" />
           <p className="text-[9px] md:text-[10px] font-black text-primary uppercase tracking-widest truncate flex-1 opacity-70">
             {globalRules.length} 条准则 · 开启继承后自动精简节点库
           </p>
         </div>
         <div className="flex items-center gap-1.5 md:gap-2 relative z-10 text-primary shrink-0 bg-primary/5 px-3 py-1.5 rounded-xl border border-primary/10 group-hover:bg-primary group-hover:text-white transition-all shadow-sm">
            <span className="text-[8px] md:text-[9px] font-black uppercase tracking-widest">配置准则</span>
            <ChevronRight className="size-3 md:size-4" />
         </div>
      </div>

      {/* Grid */}
      <div className="space-y-4 md:space-y-6 text-left px-2">
        <div className="flex items-center gap-4 opacity-60"><Layers className="size-4" /><h3 className="text-[9px] md:text-[10px] font-black uppercase tracking-[0.3em] text-left">资源矩阵</h3><div className="h-px flex-1 bg-muted" /></div>
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4 md:gap-6 text-left">
          {subs.map(sub => (
            <SubscriptionCard key={sub.id} sub={sub} onEdit={setEditingSub} onDelete={handleDelete} />
          ))}
          <button onClick={() => setEditingSub({ name: '', url: '', rules: [], traffic: {used:0, total: 100*1024**3}, interval: 360, inheritGlobal: true })} className="border-2 border-dashed rounded-[1.5rem] md:rounded-[2rem] flex flex-col items-center justify-center p-6 md:p-10 space-y-3 hover:bg-primary/[0.02] hover:border-primary/50 transition-all group min-h-[160px] md:min-h-[200px]">
            <Plus className="size-6 md:size-8 text-muted-foreground group-hover:scale-110 transition-transform" />
            <span className="text-[8px] md:text-[9px] font-black text-muted-foreground uppercase tracking-widest">Connect New</span>
          </button>
        </div>
      </div>

      {/* Editor Side Panel (Drawer) - RESPONSIBLE REDESIGN */}
      {editingSub && (
        <div className="fixed inset-0 z-50 flex justify-end overflow-hidden">
          <div className="absolute inset-0 bg-background/60 backdrop-blur-md" onClick={() => setEditingSub(null)} />
          <div className="relative w-full sm:max-w-md md:max-w-xl bg-card border-l h-full shadow-2xl flex flex-col animate-in slide-in-from-right duration-500">
            <div className="p-4 md:p-5 border-b flex justify-between items-center bg-muted/20">
              <div className="flex items-center gap-3">
                <div className="size-9 md:size-10 rounded-xl bg-primary text-primary-foreground flex items-center justify-center shadow-lg"><Activity className="size-5" /></div>
                <div><h3 className="text-base md:text-lg font-black uppercase tracking-tight">资源资产配置</h3><p className="text-[7px] md:text-[8px] font-black text-primary uppercase tracking-widest opacity-60">Resource Management</p></div>
              </div>
              <Button variant="ghost" size="icon" onClick={() => setEditingSub(null)} className="rounded-xl size-9 md:size-10 hover:bg-muted"><X className="size-5" /></Button>
            </div>
            
            <div className="flex-1 overflow-y-auto p-4 md:p-6 space-y-6 md:space-y-8 custom-scrollbar">
              {/* Basic Section - COMPACT */}
              <section className="space-y-4">
                <div className="flex items-center gap-2.5"><div className="h-6 w-1.5 bg-primary rounded-full" /><h4 className="text-sm md:text-base font-black uppercase tracking-tight">基础接入配置</h4></div>
                <div className="grid grid-cols-1 gap-4 bg-muted/10 p-4 md:p-6 rounded-2xl border border-muted shadow-inner">
                  <div className="space-y-1.5">
                    <label className="text-[7px] md:text-[8px] font-black uppercase ml-1 opacity-50 block tracking-widest">资源标识名称</label>
                    <input value={editingSub.name} onChange={e => setEditingSub({...editingSub, name: e.target.value})} placeholder="例如：飞机场主线" className="w-full bg-background border-2 border-transparent focus:border-primary/40 rounded-xl px-4 py-3 font-black outline-none transition-all shadow-sm text-sm md:text-base" />
                  </div>
                  <div className="space-y-1.5">
                    <label className="text-[7px] md:text-[8px] font-black uppercase ml-1 opacity-50 block tracking-widest">节点接口地址 (URL)</label>
                    <div className="relative">
                       <input value={editingSub.url} onChange={e => setEditingSub({...editingSub, url: e.target.value})} placeholder="https://..." className="w-full bg-background border-2 border-transparent focus:border-primary/40 rounded-xl px-4 py-3 font-mono text-[10px] outline-none transition-all shadow-sm pr-10" />
                       <Link className="absolute right-4 top-1/2 -translate-y-1/2 size-3.5 text-muted-foreground opacity-20" />
                    </div>
                  </div>
                  
                  <div className="pt-2 border-t border-dashed border-muted">
                    <button 
                      onClick={() => setEditingSub({...editingSub, inheritGlobal: !editingSub.inheritGlobal})}
                      className={cn(
                        "w-full border-2 p-3 md:p-4 rounded-xl md:rounded-2xl flex items-center justify-between transition-all group/inherit relative overflow-hidden",
                        editingSub.inheritGlobal ? "bg-primary/[0.03] border-primary/20 shadow-sm" : "bg-muted/10 border-transparent grayscale opacity-50"
                      )}
                    >
                       <div className="flex items-center gap-3">
                         <div className={cn("size-8 md:size-10 rounded-lg md:rounded-xl flex items-center justify-center shadow-lg transition-all", editingSub.inheritGlobal ? "bg-primary text-white shadow-primary/20" : "bg-zinc-500 text-white")}><Shield className="size-4 md:size-5" /></div>
                         <div className="text-left">
                           <p className={cn("font-black uppercase tracking-tight text-[10px] md:text-xs", editingSub.inheritGlobal ? "text-primary" : "text-zinc-600")}>继承通用精选准则</p>
                           <p className="text-[7px] md:text-[8px] font-bold opacity-60 uppercase tracking-wider">Common Standards</p>
                         </div>
                       </div>
                       <div className={cn("w-10 md:w-12 h-5 md:h-6 rounded-full relative transition-all shadow-inner border border-black/5", editingSub.inheritGlobal ? "bg-primary" : "bg-zinc-400")}>
                          <div className={cn("absolute top-1 size-3 md:size-4 bg-white rounded-full transition-all shadow-md", editingSub.inheritGlobal ? "right-1" : "left-1")} />
                       </div>
                    </button>
                  </div>

                  <div className="space-y-2.5 pt-2 border-t border-dashed border-muted">
                    <label className="text-[7px] md:text-[8px] font-black uppercase ml-1 opacity-50 block tracking-widest">自动同步频率</label>
                    <div className="flex flex-wrap gap-1.5">
                       {intervals.map((item) => (
                         <button key={item.value} onClick={() => setEditingSub({...editingSub, interval: item.value})}
                           className={cn("px-3 md:px-4 py-1.5 md:py-2 rounded-lg md:rounded-xl text-[8px] md:text-[10px] font-black uppercase border-2 transition-all active:scale-95",
                             editingSub.interval === item.value ? "bg-zinc-900 text-white border-zinc-900 shadow-lg" : "bg-background border-transparent text-muted-foreground hover:bg-muted")}>{item.label}</button>
                       ))}
                    </div>
                  </div>
                </div>
              </section>

              {/* ASSET COMPOSITION - RESPONSIVE TILES */}
              {editingSub.id && editingSub.breakdown && (
                <section className="space-y-4">
                  <div className="flex items-center gap-2.5"><div className="h-6 w-1.5 bg-primary/40 rounded-full" /><h4 className="text-sm md:text-base font-black uppercase tracking-tight">入池资产透视</h4></div>
                  <div className="grid grid-cols-2 md:grid-cols-3 gap-3">
                    {Object.entries(editingSub.breakdown).map(([type, count]: any) => (
                      <div key={type} className="bg-muted/10 border-2 rounded-xl md:rounded-2xl p-3 md:p-4 shadow-sm flex flex-col items-start gap-0.5 group hover:border-primary/20 transition-all border-b-4 border-b-primary/5">
                         <span className="text-[7px] md:text-[8px] font-black text-primary uppercase tracking-[0.2em] opacity-60">{type}</span>
                         <div className="flex items-baseline gap-1"><p className="text-xl md:text-2xl font-black tracking-tighter">{count}</p><span className="text-[7px] md:text-[8px] font-bold opacity-30 uppercase tracking-widest">PCS</span></div>
                      </div>
                    ))}
                  </div>
                </section>
              )}

              {/* SELECTION RULES - COMPACT BLOCKS */}
              <section className="space-y-4">
                {!showAdvanced ? (
                  <button onClick={() => setShowAdvanced(true)} className="group flex items-center gap-3 text-[8px] md:text-[9px] font-black uppercase text-primary bg-primary/5 px-6 py-3.5 rounded-xl md:rounded-2xl border-2 border-primary/10 shadow-sm transition-all hover:bg-primary/10 w-full justify-center"><Filter className="size-3.5 animate-bounce" /> 配置个体精选规则 (Advanced Rules) <ChevronDown className="size-3" /></button>
                ) : (
                  <div className="space-y-5 animate-in slide-in-from-top-4 duration-500">
                    <div className="flex items-center justify-between mb-2">
                       <div className="flex items-center gap-2.5">
                         <div className="h-6 w-1.5 bg-green-500 rounded-full shadow-[0_0_10px_rgba(34,197,94,0.3)]" />
                         <h4 className="text-sm md:text-base font-black uppercase tracking-tight">个体精选规则</h4>
                       </div>
                       <div className="flex flex-col items-end">
                         <Button onClick={handleAddRule} variant="default" className="rounded-xl font-black text-[9px] uppercase gap-1.5 border-b-4 border-green-700 bg-green-500 hover:bg-green-600 hover:border-green-800 text-white transition-all hover:translate-y-[2px] active:border-b-0 active:translate-y-[4px] px-4 h-9 shadow-lg shadow-green-500/20 group">
                           <Plus className="size-3.5 group-hover:rotate-90 transition-transform" /> 添加精选积木
                         </Button>
                         <span className="text-[7px] font-bold text-green-600/50 uppercase tracking-tighter mt-1 animate-pulse">Build your scheme</span>
                       </div>
                    </div>

                    {/* INTERACTIVE GUIDE - ONBOARDING */}
                    <div className="bg-primary/[0.03] border border-primary/10 rounded-xl p-3 flex gap-3 items-start animate-in fade-in slide-in-from-top-2 duration-700">
                       <div className="size-6 rounded-lg bg-primary/10 flex items-center justify-center shrink-0 mt-0.5">
                          <Info className="size-3.5 text-primary" />
                       </div>
                       <div className="space-y-1">
                          <p className="text-[10px] font-black text-primary uppercase tracking-wider">交互指南 / Interaction Guide</p>
                          <p className="text-[10px] font-medium text-muted-foreground leading-relaxed">
                            点击 <span className="bg-primary/10 px-1 rounded font-bold text-primary">引入/剔除</span> 切换逻辑，点击 <span className="bg-muted px-1 rounded font-bold">包含/正则</span> 切换模式。拖动输入框即可自定义匹配。
                          </p>
                       </div>
                    </div>

                    <div className="space-y-3">
                      {editingSub.rules.length === 0 && (
                        <div className="py-10 border-2 border-dashed border-muted rounded-2xl flex flex-col items-center justify-center text-center space-y-3 opacity-40">
                           <Layers className="size-8 mb-1" />
                           <div>
                             <p className="text-xs font-black uppercase tracking-widest">暂无活跃准则</p>
                             <p className="text-[9px] font-bold">点击上方按钮，开启个性化精选</p>
                           </div>
                        </div>
                      )}
                      {editingSub.rules.map((rule: any, i: number) => (
                        <div key={i} className={cn(
                          "relative rounded-2xl p-3 md:p-4 transition-all group animate-in zoom-in-95 duration-300 border-2 bg-card",
                          rule.action === 'keep' ? "border-emerald-500/20" : "border-rose-500/20"
                        )}>
                          <div className="flex items-center gap-3">
                            {/* Action Switcher */}
                            <button 
                              onClick={() => handleUpdateRule(i, { action: rule.action === 'keep' ? 'discard' : 'keep' })}
                              className={cn(
                                "flex items-center gap-2 px-3 py-1.5 rounded-xl font-black text-[10px] uppercase transition-all active:scale-95 shadow-sm",
                                rule.action === 'keep' ? "bg-emerald-500 text-white shadow-emerald-500/20" : "bg-rose-500 text-white shadow-rose-500/20"
                              )}
                            >
                              {rule.action === 'keep' ? <CheckCircle2 className="size-3.5" /> : <ZapOff className="size-3.5" />}
                              {rule.action === 'keep' ? '引入' : '剔除'}
                            </button>

                            <div className="flex-1 flex items-center gap-2 overflow-hidden">
                               <span className="text-[10px] font-bold text-muted-foreground shrink-0">节点名</span>
                               
                               {/* Match Type Dropdown-style Button */}
                               <button 
                                 onClick={() => handleCycleRuleType(i)}
                                 className={cn(
                                   "flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg border-2 text-[10px] font-black transition-all hover:bg-muted active:scale-95 shrink-0",
                                   rule.type === 'regex' ? "bg-zinc-900 border-zinc-800 text-emerald-400 font-mono shadow-inner" : 
                                   rule.type === 'not_contains' ? "bg-amber-50 border-amber-200 text-amber-600 shadow-sm" : 
                                   "bg-background border-muted text-muted-foreground shadow-sm"
                                 )}
                               >
                                 {rule.type === 'regex' && <span className="opacity-50 text-[8px]">.*</span>}
                                 {rule.type === 'not_contains' && <ShieldAlert className="size-3" />}
                                 {rule.type === 'regex' ? '正则匹配' : rule.type === 'not_contains' ? '不包含' : '包含'}
                                 <ChevronDown className="size-2.5 opacity-30 ml-0.5" />
                               </button>

                               {/* Pattern Input - PROMINENT SLOT DESIGN */}
                               <div className="flex-1 relative min-w-[140px] group/input">
                                 <input 
                                   autoFocus={lastAddedIndex === i}
                                   placeholder={rule.type === 'regex' ? '(?i)HongKong' : '关键字...'}
                                   value={rule.pattern} 
                                   onChange={(e) => handleUpdateRule(i, { pattern: e.target.value })}
                                   className={cn(
                                     "w-full bg-background border-2 rounded-xl px-3 py-2 text-[12px] font-black transition-all outline-none shadow-inner",
                                     "placeholder:text-muted-foreground/30",
                                     rule.type === 'regex' ? "font-mono text-emerald-700 border-zinc-800 focus:border-emerald-500 focus:ring-4 ring-emerald-500/10" : 
                                     "border-muted-foreground/10 focus:border-primary focus:ring-4 ring-primary/10 text-foreground"
                                   )}
                                 />
                                 <div className="absolute right-3 top-1/2 -translate-y-1/2 opacity-20 pointer-events-none group-focus-within/input:opacity-100 transition-opacity">
                                    <div className="size-1.5 rounded-full bg-primary animate-pulse" />
                                 </div>
                               </div>
                            </div>

                            {/* Delete Action */}
                            <button 
                              onClick={() => handleDeleteRule(i)} 
                              className="size-8 rounded-xl bg-muted/50 text-muted-foreground opacity-0 group-hover:opacity-100 transition-all hover:bg-rose-500 hover:text-white flex items-center justify-center shrink-0 shadow-sm"
                            >
                              <Trash2 className="size-4" />
                            </button>
                          </div>
                          
                          {/* Left-side subtle status indicator */}
                          <div className={cn(
                            "absolute left-0 top-1/2 -translate-y-1/2 w-1 h-8 rounded-r-full transition-all",
                            rule.action === 'keep' ? "bg-emerald-500" : "bg-rose-500"
                          )} />
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </section>
            </div>

            {/* Footer Buttons - ADAPTIVE */}
            <div className="p-4 md:p-6 border-t bg-muted/10 grid grid-cols-2 gap-4 shadow-[0_-10px_30px_rgba(0,0,0,0.05)]">
              <Button variant="outline" onClick={() => setEditingSub(null)} className="rounded-xl md:rounded-2xl h-12 md:h-14 font-black uppercase border-2 text-[9px] md:text-xs tracking-widest hover:bg-background transition-all">放弃修改</Button>
              <Button onClick={handleUpdateSub} className="rounded-xl md:rounded-2xl h-12 md:h-14 bg-zinc-900 hover:bg-black text-white font-black uppercase shadow-xl shadow-black/20 text-[9px] md:text-xs tracking-widest transition-all hover:scale-105 active:scale-95">保存配置同步</Button>
            </div>
          </div>
        </div>
      )}

      {/* GLOBAL RULES DRAWER - ALSO REFACTORED FOR RESPONSIVENESS */}
      {isGlobalDrawerOpen && (
        <div className="fixed inset-0 z-50 flex justify-end overflow-hidden">
          <div className="absolute inset-0 bg-background/60 backdrop-blur-md" onClick={() => setIsGlobalDrawerOpen(false)} />
          <div className="relative w-full sm:max-w-md md:max-w-xl bg-card border-l h-full shadow-2xl flex flex-col animate-in slide-in-from-right duration-500">
            <div className="p-5 border-b flex justify-between items-center bg-muted/20">
              <div className="flex items-center gap-4">
                <div className="size-10 rounded-xl bg-primary text-white flex items-center justify-center shadow-lg"><Shield className="size-5" /></div>
                <div><h3 className="text-lg font-black uppercase tracking-tight">通用精选准则管理</h3><p className="text-[9px] font-black text-primary uppercase tracking-widest opacity-60">Global Standards</p></div>
              </div>
              <Button variant="ghost" size="icon" onClick={() => setIsGlobalDrawerOpen(false)} className="rounded-xl size-10"><X className="size-5" /></Button>
            </div>
            <div className="flex-1 overflow-y-auto p-5 md:p-8 space-y-6 custom-scrollbar">
              <div className="bg-primary/5 border border-primary/20 p-4 rounded-2xl flex gap-4">
                 <Info className="size-5 text-primary shrink-0 mt-0.5" />
                 <p className="text-xs font-medium text-primary/80 leading-relaxed">最高优先级的全局过滤器，在导入前进行初次筛查。</p>
              </div>
              <div className="space-y-4">
                 <div className="flex justify-between items-center"><span className="text-[10px] font-black uppercase text-muted-foreground tracking-widest ml-1">有效准则</span><Button variant="outline" size="sm" className="rounded-lg font-black text-[8px] uppercase border-2 h-8">添加</Button></div>
                 {globalRules.map(rule => (
                    <div key={rule.id} className="bg-background border-2 border-muted rounded-xl p-4 shadow-sm group border-l-4 border-l-red-500">
                       <div className="flex items-center gap-4">
                         <div className="size-8 md:size-10 rounded-lg md:rounded-xl bg-red-500 text-white flex items-center justify-center shrink-0"><ZapOff className="size-4 md:size-5" /></div>
                         <div className="flex-1 text-[11px] md:text-xs font-bold">全局 <span className="text-red-600 uppercase">剔除</span> 包含 <span className="bg-zinc-100 px-1.5 py-0.5 rounded font-mono text-primary mx-0.5">"{rule.pattern}"</span> 的节点</div>
                         <button className="opacity-0 group-hover:opacity-100 p-2 text-destructive transition-all"><Trash2 className="size-4" /></button>
                       </div>
                    </div>
                 ))}
              </div>
            </div>
            <div className="p-6 border-t bg-muted/10"><Button onClick={() => setIsGlobalDrawerOpen(false)} className="w-full h-14 rounded-xl font-black uppercase shadow-xl shadow-primary/30 tracking-widest transition-all hover:scale-[1.02] active:scale-95">保存并应用</Button></div>
          </div>
        </div>
      )}
    </div>
  );
};
