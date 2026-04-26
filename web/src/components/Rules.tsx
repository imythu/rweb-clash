import { useState, useEffect, useMemo } from 'react';
import { 
  Search, Plus, Shield, Zap, Trash2, Loader2, 
  CheckCircle2, FlaskConical, Layers, Settings2, 
  Radio, Terminal, Info,
  Compass, ShieldCheck, Activity, LayoutGrid, Play,
  Network, MoveRight, X, ArrowDownRight, Fingerprint, ShieldAlert,
  Edit3, RotateCcw, Database, RefreshCw, Link as LinkIcon,
  ChevronRight, ArrowRight, MousePointer2, Clock, Globe, ChevronsRight,
  HardDrive, Cpu
} from 'lucide-react';
import { cn, SUB_DELIMITER } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { useToast } from './Toast';

// --- Nano Components ---

const SubBadge = ({ name }: { name: string }) => (
  <span className="px-1.5 py-0.5 rounded-md bg-muted/80 border border-border text-[8px] font-black text-muted-foreground uppercase tracking-tighter shrink-0 ml-1.5">
    {name}
  </span>
);

const ActionPill = ({ action, className }: { action: string, className?: string }) => {
  const isProxy = !['DIRECT', 'REJECT'].includes(action.toUpperCase());
  const isDirect = action.toUpperCase() === 'DIRECT';
  const [displayName, subName] = action.split(SUB_DELIMITER);
  
  return (
    <div className={cn(
        "px-3 py-1.5 rounded-xl border flex items-center gap-2.5 shadow-sm min-w-0 h-9",
        isDirect ? "bg-green-500/5 border-green-500/20 text-green-600" :
        isProxy ? "bg-primary/5 border-primary/20 text-primary" : 
        "bg-rose-500/5 border-rose-500/20 text-rose-600",
        className
      )}>
      <div className={cn("size-1.5 rounded-full shrink-0", isDirect ? "bg-green-500" : isProxy ? "bg-primary" : "bg-rose-500")} />
      <div className="flex items-center min-w-0 overflow-hidden">
        <span className="text-[11px] font-black uppercase truncate">{displayName}</span>
        {subName && <SubBadge name={subName} />}
      </div>
    </div>
  );
};

const LogicFlow = ({ policy }: { policy: string }) => {
  const isDirect = policy.toUpperCase() === 'DIRECT';
  const isReject = policy.toUpperCase() === 'REJECT';
  const colorClass = isDirect ? "text-green-500" : isReject ? "text-rose-500" : "text-primary";
  
  return (
    <div className={cn("flex items-center gap-0 opacity-20 w-full px-2", colorClass)}>
       <div className="flex-1 h-px bg-current" />
       <ChevronRight className="size-3.5 -ml-1 text-current shrink-0" />
    </div>
  );
};

// --- Drawers ---

const RuleSetDrawer = ({ isOpen, onClose, ruleSets, onRefresh, onDelete, onAdd }: any) => {
  const [isAdding, setIsAdding] = useState(false);
  const [newName, setNewName] = useState('');
  const [newUrl, setNewUrl] = useState('');
  const [newInterval, setNewInterval] = useState('86400');
  const [syncing, setSyncing] = useState<string | null>(null);

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-[110] flex justify-end overflow-hidden">
      <div className="absolute inset-0 bg-background/60 backdrop-blur-md animate-in fade-in duration-500" onClick={onClose} />
      <div className="relative w-full max-w-xl bg-card border-l-2 border-primary/10 shadow-2xl flex flex-col h-full animate-in slide-in-from-right duration-500 text-left">
        <div className="p-6 md:p-8 border-b flex justify-between items-center bg-muted/5 shrink-0">
           <div className="text-left"><h3 className="text-xl font-black uppercase tracking-tight flex items-center gap-3"><Database className="size-5 text-primary" /> 规则集订阅库</h3></div>
           <Button variant="ghost" size="icon" onClick={onClose} className="rounded-xl"><X className="size-6" /></Button>
        </div>
        <div className="flex-1 overflow-y-auto p-6 md:p-8 space-y-6 custom-scrollbar">
          <Button onClick={() => setIsAdding(!isAdding)} variant="outline" className="w-full h-12 rounded-xl border-dashed font-black uppercase text-[10px] tracking-widest gap-2">{isAdding ? '取消' : '添加新的规则集订阅'}</Button>
          {isAdding && (
            <div className="bg-muted/10 border-2 rounded-2xl p-5 space-y-4 animate-in slide-in-from-top-4">
               <div className="space-y-1.5"><label className="text-[9px] font-black uppercase opacity-40">订阅名称</label><input value={newName} onChange={e => setNewName(e.target.value)} placeholder="e.g. gfw_list" className="w-full bg-background border-2 border-muted rounded-xl px-4 py-3 text-xs font-black" /></div>
               <div className="space-y-1.5"><label className="text-[9px] font-black uppercase opacity-40">资源 URL</label><input value={newUrl} onChange={e => setNewUrl(e.target.value)} placeholder="https://..." className="w-full bg-background border-2 border-muted rounded-xl px-4 py-3 text-[10px] font-mono" /></div>
               <Button onClick={() => { onAdd({ name: newName, url: newUrl, interval: parseInt(newInterval) }); setIsAdding(false); setNewName(''); setNewUrl(''); }} className="w-full h-11 bg-primary text-primary-foreground rounded-xl font-black uppercase text-[10px]">确认订阅</Button>
            </div>
          )}
          <div className="space-y-3">
            {ruleSets.map((rs: any) => (
              <div key={rs.id} className="bg-muted/5 border-2 rounded-2xl p-4 group hover:border-primary/20 transition-all text-left text-foreground">
                <div className="flex justify-between items-start mb-3">
                   <div className="min-w-0 flex-1"><h5 className="text-xs font-black uppercase truncate">{rs.name}</h5><p className="text-[9px] font-mono text-muted-foreground truncate opacity-60">{rs.url}</p></div>
                   <div className="flex gap-1.5 opacity-0 group-hover:opacity-100 transition-all"><Button variant="ghost" size="icon" onClick={async () => { setSyncing(rs.id); await onRefresh(rs.id); setSyncing(null); }} className="size-7"><RefreshCw className={cn("size-3.5", syncing === rs.id && "animate-spin")} /></Button><Button variant="ghost" size="icon" onClick={() => onDelete(rs.id)} className="size-7 text-red-500"><Trash2 className="size-3.5" /></Button></div>
                </div>
                <div className="flex gap-4 text-[8px] font-black uppercase opacity-40"><span>Entries: {rs.ruleCount || 0}</span><span>Update: {rs.lastUpdate}</span></div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
};

const RuleDrawer = ({ isOpen, onClose, onSave, proxies, nodes, ruleSets, initialData }: any) => {
  const [type, setType] = useState('DOMAIN-SUFFIX');
  const [value, setValue] = useState('');
  const [policy, setPolicy] = useState('DIRECT');
  const [search, setSearch] = useState('');
  const { toast } = useToast();

  useEffect(() => {
    if (initialData && isOpen) {
      setType(initialData.type || 'DOMAIN-SUFFIX'); setValue(initialData.value || ''); setPolicy(initialData.policy || 'DIRECT');
    } else if (isOpen) {
      setType('DOMAIN-SUFFIX'); setValue(''); setPolicy('DIRECT');
    }
  }, [initialData, isOpen]);

  const targets = useMemo(() => {
    const list = [
      { name: 'DIRECT', type: 'system', icon: Zap }, { name: 'REJECT', type: 'system', icon: ShieldAlert },
      ...proxies.map((p: any) => ({ name: p.name, type: 'group', icon: Layers })),
      ...nodes.map((n: any) => ({ name: n.name, type: 'node', icon: MousePointer2 }))
    ];
    return list.filter(t => t.name.toLowerCase().includes(search.toLowerCase()));
  }, [proxies, nodes, search]);

  const handleSave = () => {
    if (!value && type !== 'MATCH') return toast('参数缺失', 'error');
    onSave({ type, value: type === 'MATCH' ? 'ANY' : value, policy });
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-[110] flex justify-end overflow-hidden">
      <div className="absolute inset-0 bg-background/40 backdrop-blur-sm animate-in fade-in duration-500" onClick={onClose} />
      <div className="relative w-full max-w-md bg-card border-l h-full shadow-2xl flex flex-col animate-in slide-in-from-right duration-500 ease-[cubic-bezier(0.32,0.72,0,1)] text-left text-foreground">
        <div className="p-6 border-b flex justify-between items-center bg-muted/5">
           <h3 className="text-lg font-black uppercase tracking-tight">{initialData ? '修改规则' : '新增规则'}</h3>
           <Button variant="ghost" size="icon" onClick={onClose} className="rounded-lg"><X className="size-5" /></Button>
        </div>
        <div className="flex-1 overflow-y-auto p-6 space-y-10 custom-scrollbar">
           <section className="space-y-4">
              <div className="flex items-center gap-2 opacity-30 text-[9px] font-black uppercase"><Fingerprint className="size-3" /> 01. 捕获</div>
              <div className="space-y-4">
                 <select value={type} onChange={e => { setType(e.target.value); setValue(''); }} className="w-full bg-background border-2 border-muted rounded-xl px-4 py-3.5 text-sm font-black outline-none focus:border-primary/40"><option value="RULE-SET">规则集</option><option value="DOMAIN-SUFFIX">域名后缀</option><option value="DOMAIN">精确域名</option><option value="IP-CIDR">IP 分段</option><option value="GEOIP">国家/地区</option><option value="MATCH">默认兜底</option></select>
                 {type === 'RULE-SET' ? (
                   <select value={value} onChange={e => setValue(e.target.value)} className="w-full bg-background border-2 border-muted rounded-xl px-4 py-3.5 text-sm font-black outline-none focus:border-primary/40">
                      <option value="">选择规则集...</option>{ruleSets.map((rs: any) => <option key={rs.id} value={rs.name}>{rs.name}</option>)}
                   </select>
                 ) : (
                   <input value={value} onChange={e => setValue(e.target.value)} disabled={type === 'MATCH'} placeholder="e.g. google.com" className="w-full bg-background border-2 border-muted rounded-xl px-4 py-3.5 text-sm font-black outline-none focus:border-primary/40" />
                 )}
              </div>
           </section>
           <section className="space-y-4">
              <div className="flex items-center gap-2 opacity-30 text-[9px] font-black uppercase"><ArrowDownRight className="size-3" /> 02. 指派</div>
              <div className="relative"><Search className="absolute left-4 top-1/2 -translate-y-1/2 size-4 text-muted-foreground opacity-40" /><input value={search} onChange={e => setSearch(e.target.value)} placeholder="检索出口引擎或物理节点..." className="w-full pl-11 pr-4 py-3.5 bg-muted/10 border rounded-xl text-xs font-black outline-none focus:border-primary/40 shadow-inner" /></div>
              <div className="grid grid-cols-1 gap-2 max-h-[300px] overflow-y-auto custom-scrollbar">
                 {targets.map(t => {
                   const [dName, sName] = t.name.split(SUB_DELIMITER);
                   return (
                     <button key={t.name} onClick={() => setPolicy(t.name)} className={cn("flex items-center justify-between px-4 py-3 rounded-xl border transition-all", policy === t.name ? "bg-primary/10 border-primary" : "bg-muted/5 border-transparent hover:bg-muted/10")}>
                       <div className="flex items-center gap-3"><t.icon className="size-4 opacity-40" /><div className="flex items-center gap-2 text-xs font-black uppercase">{dName} {sName && <SubBadge name={sName} />}</div></div>
                       {policy === t.name && <CheckCircle2 className="size-4 text-primary animate-in zoom-in" />}
                     </button>
                   );
                 })}
              </div>
           </section>
        </div>
        <div className="p-6 border-t flex gap-4 bg-muted/5">
           <Button variant="ghost" onClick={onClose} className="flex-1 h-12 rounded-xl text-xs font-black uppercase">取消</Button>
           <Button onClick={handleSave} className="flex-[2] h-12 bg-primary text-primary-foreground rounded-xl text-xs font-black uppercase shadow-lg shadow-primary/20">保存路由配置</Button>
        </div>
      </div>
    </div>
  );
};

// --- Main Page ---

export const Rules = () => {
  const { toast } = useToast();
  const [rules, setRules] = useState<any[]>([]);
  const [ruleSets, setRuleSets] = useState<any[]>([]);
  const [proxies, setProxies] = useState<any[]>([]);
  const [nodes, setNodes] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const [search, setSearch] = useState('');
  const [isDrawerOpen, setIsDrawerOpen] = useState(false);
  const [isRSOpen, setIsRSOpen] = useState(false);
  const [isLabOpen, setIsLabOpen] = useState(false);
  const [editingRule, setEditingRule] = useState<any>(null);
  const [testTarget, setTarget] = useState('');
  const [testResult, setTestResult] = useState<any>(null);
  const [isTesting, setIsTesting] = useState(false);

  useEffect(() => { 
    const init = async () => {
      try {
        const [rRes, pRes, rsRes] = await Promise.all([fetch('/api/rules'), fetch('/api/proxies'), fetch('/api/rule-sets')]);
        const [rData, pData, rsData] = await Promise.all([rRes.json(), pRes.json(), rsRes.json()]);
        setRules(rData); setProxies(pData.groups || []); setNodes(pData.nodes || []); setRuleSets(rsData);
      } finally { setLoading(false); }
    };
    init();
  }, []);

  const fetchRules = async () => { const res = await fetch('/api/rules'); const data = await res.json(); setRules(data); };
  const fetchRuleSets = async () => { const res = await fetch('/api/rule-sets'); const data = await res.json(); setRuleSets(data); };

  const handleSaveRule = async (data: any) => {
    try {
      const method = editingRule ? 'PUT' : 'POST';
      const url = editingRule ? `/api/rules/` + editingRule.id : '/api/rules';
      await fetch(url, { method, body: JSON.stringify(data) });
      toast('Success', 'success'); setIsDrawerOpen(false); setEditingRule(null); fetchRules();
    } catch (e) { toast('Error', 'error'); }
  };

  const handleTestSandbox = async () => {
    if (!testTarget) return;
    setIsTesting(true);
    try {
      const res = await fetch('/api/rules/test', { method: 'POST', body: JSON.stringify({ target: testTarget }) });
      const data = await res.json(); setTestResult(data);
    } finally { setIsTesting(false); }
  };

  const filteredRules = useMemo(() => {
    return rules.filter(r => r.value.toLowerCase().includes(search.toLowerCase()) || r.policy.toLowerCase().includes(search.toLowerCase()) || r.type.toLowerCase().includes(search.toLowerCase()));
  }, [rules, search]);

  if (loading) return <div className="flex items-center justify-center h-[60vh]"><Loader2 className="animate-spin text-primary size-8" /></div>;

  const GRID_COLS = "lg:grid-cols-[theme(width.12)_theme(width.32)_1fr_theme(width.20)_theme(width.48)_theme(width.24)]";

  return (
    <div className="max-w-[1600px] mx-auto space-y-6 md:space-y-8 animate-in fade-in duration-700 px-3 md:px-4 pb-24 text-left text-foreground">
      
      {/* 1. Header */}
      <div className="flex flex-col md:flex-row justify-between items-start md:items-end gap-6 px-1">
        <div className="text-left">
           <h2 className="text-2xl md:text-3xl font-black uppercase tracking-tight">路由决策中心</h2>
           <p className="text-[9px] font-black text-muted-foreground uppercase mt-1 tracking-widest opacity-60">Logic Hub Console</p>
        </div>
        <div className="flex items-center gap-3 w-full md:w-auto text-left">
           <Button onClick={() => setIsRSOpen(true)} variant="outline" className="flex-1 md:flex-none h-11 px-6 rounded-xl font-black uppercase text-[9px] border-2 shadow-sm gap-2"><Database className="size-4 text-primary" /> 规则集库</Button>
           <Button onClick={() => { setEditingRule(null); setIsDrawerOpen(true); }} className="flex-1 md:flex-none h-11 px-8 rounded-xl font-black uppercase text-[9px] shadow-lg shadow-primary/20 gap-2 bg-primary text-primary-foreground transition-all hover:scale-105 active:scale-95"><Plus className="size-4" /> 新增决策</Button>
        </div>
      </div>

      {/* 2. Control Bar */}
      <div className="bg-card/60 backdrop-blur-xl border border-border/40 p-3 md:p-4 rounded-3xl shadow-sm flex flex-col md:flex-row items-center gap-4 text-left">
         <div className="relative flex-1 w-full">
            <Search className="absolute left-4 top-1/2 -translate-y-1/2 size-4 text-muted-foreground opacity-30" />
            <input value={search} onChange={e => setSearch(e.target.value)} placeholder="检索活跃分流记录..." className="w-full pl-11 pr-4 py-3 bg-background border border-muted rounded-2xl text-[11px] font-black uppercase outline-none focus:border-primary shadow-inner text-foreground" />
         </div>
         <div className="flex items-center gap-3 shrink-0 text-left">
            <Button onClick={() => setIsLabOpen(true)} variant="outline" className="h-11 px-6 rounded-xl font-black uppercase text-[9px] gap-2 bg-zinc-950 text-white hover:bg-black shadow-lg shadow-black/20"><FlaskConical className="size-4 text-blue-400" /> 追踪实验室</Button>
            <Button variant="outline" size="icon" onClick={() => { fetchRules(); fetchRuleSets(); }} className="size-11 rounded-xl border-2"><RotateCcw className="size-5" /></Button>
         </div>
      </div>

      {/* 3. The Table Header (Desktop Only) */}
      <div className={cn("hidden lg:grid gap-4 px-6 text-[9px] font-black uppercase text-muted-foreground opacity-40 tracking-[0.2em] select-none", GRID_COLS)}>
         <div className="text-center">Pos</div>
         <div>Logic Type</div>
         <div>Match Pattern</div>
         <div className="text-center">Flow</div>
         <div>Target Action</div>
         <div className="text-right pr-4">Ops</div>
      </div>

      {/* 4. The Data Grid */}
      <div className="space-y-2">
        {filteredRules.map((rule, idx) => (
          <div key={rule.id} className="group relative bg-card/60 hover:bg-card border border-border/60 hover:border-primary/20 rounded-2xl p-3 md:px-6 md:py-4 transition-all duration-300 shadow-sm overflow-hidden text-left">
             {/* Desktop Layout */}
             <div className={cn("hidden lg:grid items-center gap-4", GRID_COLS)}>
                <div className="text-center font-mono text-[10px] opacity-30 font-black">{String(idx + 1).padStart(2, '0')}</div>
                <div><div className="px-2 py-0.5 rounded bg-zinc-900 text-zinc-100 text-[8px] font-black uppercase tracking-widest inline-block border border-white/5">{rule.type}</div></div>
                <div className="min-w-0"><h4 className="text-sm font-black truncate text-foreground">{rule.value}</h4><p className="text-[8px] font-bold text-muted-foreground uppercase opacity-50 truncate">{rule.desc || 'Active Dispatching'}</p></div>
                <div className="flex justify-center"><LogicFlow policy={rule.policy} /></div>
                <div className="flex justify-start min-w-0"><ActionPill action={rule.policy} className="w-full max-w-[180px]" /></div>
                <div className="flex items-center justify-end gap-1.5 opacity-0 group-hover:opacity-100 transition-all pr-2">
                   <Button onClick={() => { setEditingRule(rule); setIsDrawerOpen(true); }} variant="ghost" size="icon" className="size-8 rounded-lg hover:bg-muted"><Edit3 className="size-3.5" /></Button>
                   <Button onClick={() => fetch(`/api/rules/${rule.id}`, { method: 'DELETE' }).then(fetchRules)} variant="ghost" size="icon" className="size-8 rounded-lg text-red-500 hover:bg-red-500/10"><Trash2 className="size-3.5" /></Button>
                </div>
             </div>

             {/* Mobile Layout */}
             <div className="flex lg:hidden flex-col gap-4 text-left">
                <div className="flex items-start justify-between gap-4 text-left">
                   <div className="flex gap-3 min-w-0 text-left">
                      <span className="text-[10px] font-mono opacity-20 font-black pt-1">{String(idx + 1).padStart(2, '0')}</span>
                      <div className="min-w-0 text-left"><div className="px-1.5 py-0.5 rounded bg-zinc-900 text-zinc-100 text-[7px] font-black uppercase tracking-widest mb-1.5 inline-block">{rule.type}</div><h4 className="text-sm font-black truncate text-foreground">{rule.value}</h4></div>
                   </div>
                   <div className="flex gap-1 shrink-0"><Button onClick={() => { setEditingRule(rule); setIsDrawerOpen(true); }} variant="ghost" size="icon" className="size-8 rounded-lg bg-muted/50"><Edit3 className="size-3.5" /></Button><Button onClick={() => fetch(`/api/rules/${rule.id}`, { method: 'DELETE' }).then(fetchRules)} variant="ghost" size="icon" className="size-8 rounded-lg text-red-500 bg-red-500/5"><Trash2 className="size-3.5" /></Button></div>
                </div>
                <div className="flex items-center gap-3 pt-3 border-t border-dashed border-muted/50"><div className="flex-1 min-w-0"><ActionPill action={rule.policy} className="w-full" /></div></div>
             </div>

             {/* Dynamic Accent Accent - Fixed to clean thin line */}
             <div className={cn("absolute left-0 inset-y-3 w-0.5 rounded-r-full transition-all duration-500 opacity-60", rule.policy === 'DIRECT' ? "bg-green-500" : rule.policy === 'REJECT' ? "bg-rose-500" : "bg-primary")} />
          </div>
        ))}
        {filteredRules.length === 0 && <div className="py-32 border-4 border-dashed border-muted rounded-[3rem] flex flex-col items-center justify-center opacity-10 text-foreground text-left"><LayoutGrid className="size-16 mb-4" /><p className="text-xl font-black uppercase tracking-widest">Empty Logic Deck</p></div>}
      </div>

      {/* Lab Modal */}
      {isLabOpen && (
        <div className="fixed inset-0 z-[120] flex items-center justify-center p-4">
           <div className="absolute inset-0 bg-background/60 backdrop-blur-xl" onClick={() => setIsLabOpen(false)} />
           <div className="relative w-full max-w-lg bg-zinc-950 text-white rounded-[3rem] p-8 md:p-12 shadow-2xl border-4 border-white/5 animate-in zoom-in-95 text-left">
              <div className="flex justify-between items-start mb-10 text-left">
                <div className="flex items-center gap-5 text-left">
                  <div className="size-16 rounded-[1.5rem] bg-blue-600 flex items-center justify-center shadow-2xl shadow-blue-600/40 shrink-0"><FlaskConical className="size-8" /></div>
                  <div className="text-left">
                    <h3 className="text-2xl font-black uppercase tracking-tight">追踪实验室</h3>
                    <p className="text-[10px] font-black text-blue-400/60 uppercase tracking-[0.3em]">Trace Sandbox</p>
                  </div>
                </div>
                <Button variant="ghost" size="icon" onClick={() => setIsLabOpen(false)} className="text-white/30 hover:text-white rounded-xl"><X className="size-6" /></Button>
              </div>
              <div className="space-y-8 text-left">
                 <div className="space-y-2 text-left"><label className="text-[9px] font-black uppercase ml-1 text-zinc-500 tracking-widest text-left">目标地址 (Domain/IP)</label><input value={testTarget} onChange={e => setTarget(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && handleTestSandbox()} placeholder="e.g. netflix.com" className="w-full px-6 py-5 bg-white/5 border-2 border-white/10 rounded-2xl text-xl font-black outline-none focus:border-blue-500 text-white" /></div>
                 <Button disabled={isTesting || !testTarget} onClick={handleTestSandbox} className="w-full h-16 bg-blue-600 hover:bg-blue-500 text-white rounded-[1.5rem] font-black uppercase text-xs tracking-widest active:scale-95 transition-all gap-4">{isTesting ? <Loader2 className="animate-spin size-6" /> : <Play className="size-6 fill-current" />} 执行路径分析</Button>
                 {testResult && (
                   <div className="pt-8 border-t border-white/5 space-y-8 text-left">
                      {[
                        { label: 'HIT RULE', val: `${testResult.hitRule.type}: ${testResult.hitRule.value}`, icon: Shield, col: "text-zinc-500" }, 
                        { label: 'TARGET POLICY', val: testResult.hitRule.policy, icon: Layers, col: "text-primary" }, 
                        { label: 'EXIT NODE', val: testResult.finalProxy, icon: CheckCircle2, col: "text-green-500" }
                      ].map((step, i) => {
                         const [d, s] = step.val.split(SUB_DELIMITER);
                         return (
                           <div key={i} className="flex items-center gap-6 text-left">
                              <div className="size-14 rounded-2xl bg-white/5 border border-white/10 flex items-center justify-center shrink-0"><step.icon className={cn("size-7", step.col)} /></div>
                              <div className="min-w-0 text-left">
                                 <p className="text-[9px] font-black text-zinc-500 uppercase tracking-widest mb-1.5">{step.label}</p>
                                 <div className="flex items-center gap-2">
                                    <p className={cn("text-xl font-black truncate", step.col)}>{d}</p>
                                    {s && <SubBadge name={s} />}
                                 </div>
                              </div>
                           </div>
                         );
                      })}
                   </div>
                 )}
              </div>
           </div>
        </div>
      )}

      <RuleDrawer isOpen={isDrawerOpen} onClose={() => setIsDrawerOpen(false)} onSave={handleSaveRule} proxies={proxies} nodes={nodes} ruleSets={ruleSets} initialData={editingRule} />
      <RuleSetDrawer isOpen={isRSOpen} onClose={() => setIsRSOpen(false)} ruleSets={ruleSets} onRefresh={fetchRuleSets} onDelete={(id: string) => fetch(`/api/rule-sets/${id}`, { method: 'DELETE' }).then(fetchRuleSets)} onAdd={(data: any) => fetch('/api/rule-sets', { method: 'POST', body: JSON.stringify(data) }).then(fetchRuleSets)} />
    </div>
  );
};
