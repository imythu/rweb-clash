import { useState, useEffect, useMemo } from 'react';
import { 
  Search, Plus, Shield, Zap, Trash2, Loader2, 
  CheckCircle2, FlaskConical, Layers,
  LayoutGrid, Play,
  X, ArrowDownRight, Fingerprint, ShieldAlert,
  Edit3, RotateCcw, Database, RefreshCw,
  ChevronRight, MousePointer2,
  GripVertical, Pin,
  type LucideIcon
} from 'lucide-react';
import { cn, SUB_DELIMITER } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { useToast } from './toast-context';
import { api, type ProxyGroup, type ProxyNode, type Rule, type RuleInput, type RuleSet, type RuleSetBehavior, type RuleSetInput, type RuleTestResult } from '@/lib/api';

// --- Nano Components ---

type RuleSetDrawerProps = {
  isOpen: boolean;
  onClose: () => void;
  ruleSets: RuleSet[];
  onRefresh: (id: string) => void | Promise<void>;
  onDelete: (id: string) => void | Promise<void>;
  onAdd: (input: RuleSetInput) => Promise<boolean>;
};

type RuleDrawerProps = {
  isOpen: boolean;
  onClose: () => void;
  onSave: (input: RuleInput) => void | Promise<void>;
  proxies: ProxyGroup[];
  nodes: ProxyNode[];
  ruleSets: RuleSet[];
  initialData: Rule | null;
};

type RuleTarget = {
  name: string;
  displayName: string;
  subscriptionName: string | null;
  type: 'system' | 'group' | 'node';
  icon: LucideIcon;
};

const SubBadge = ({ name }: { name: string }) => (
  <span className="px-1.5 py-0.5 rounded-md bg-muted border border-border text-[9px] font-black text-muted-foreground uppercase tracking-tight shrink-0 ml-1.5">
    {name}
  </span>
);

const displayRuntimeName = (name: string, displayName?: string | null) =>
  displayName || name.split(SUB_DELIMITER)[0] || name;

const ActionPill = ({ action, displayName, subscriptionName, className }: { action: string, displayName?: string | null, subscriptionName?: string | null, className?: string }) => {
  const isProxy = !['DIRECT', 'REJECT'].includes(action.toUpperCase());
  const isDirect = action.toUpperCase() === 'DIRECT';
  const label = displayRuntimeName(action, displayName);
  
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
        <span className="text-[11px] font-black uppercase truncate">{label}</span>
        {subscriptionName && <SubBadge name={subscriptionName} />}
      </div>
    </div>
  );
};

const LogicFlow = ({ policy }: { policy: string }) => {
  const isDirect = policy.toUpperCase() === 'DIRECT';
  const isReject = policy.toUpperCase() === 'REJECT';
  const colorClass = isDirect ? "text-green-500" : isReject ? "text-rose-500" : "text-primary";
  
  return (
    <div className={cn("flex items-center gap-0 opacity-55 w-full px-2", colorClass)}>
       <div className="flex-1 h-px bg-current" />
       <ChevronRight className="size-3.5 -ml-1 text-current shrink-0" />
    </div>
  );
};

// --- Drawers ---

const RuleSetDrawer = ({ isOpen, onClose, ruleSets, onRefresh, onDelete, onAdd }: RuleSetDrawerProps) => {
  const [isAdding, setIsAdding] = useState(false);
  const [newName, setNewName] = useState('');
  const [newUrl, setNewUrl] = useState('');
  const [newBehavior, setNewBehavior] = useState<RuleSetBehavior>('classical');
  const newInterval = '86400';
  const [syncing, setSyncing] = useState<string | null>(null);
  const [isSaving, setIsSaving] = useState(false);
  const { toast } = useToast();

  const handleAdd = async () => {
    if (!newName.trim() || !newUrl.trim()) {
      toast('请完整填写订阅名称和资源 URL', 'error');
      return;
    }
    setIsSaving(true);
    try {
      const saved = await onAdd({ name: newName.trim(), url: newUrl.trim(), interval: parseInt(newInterval), behavior: newBehavior });
      if (saved) {
        setIsAdding(false);
        setNewName('');
        setNewUrl('');
        setNewBehavior('classical');
      }
    } finally {
      setIsSaving(false);
    }
  };

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
               <div className="space-y-1.5"><label className="text-[10px] font-black uppercase text-muted-foreground">订阅名称</label><input value={newName} onChange={e => setNewName(e.target.value)} placeholder="e.g. gfw_list" className="w-full bg-background border-2 border-muted rounded-xl px-4 py-3 text-xs font-black" /></div>
               <div className="space-y-1.5"><label className="text-[10px] font-black uppercase text-muted-foreground">资源 URL</label><input value={newUrl} onChange={e => setNewUrl(e.target.value)} placeholder="https://..." className="w-full bg-background border-2 border-muted rounded-xl px-4 py-3 text-[10px] font-mono" /></div>
               <div className="space-y-1.5"><label className="text-[10px] font-black uppercase text-muted-foreground">Behavior</label><select value={newBehavior} onChange={event => setNewBehavior(event.target.value as RuleSetBehavior)} className="w-full bg-background border-2 border-muted rounded-xl px-4 py-3 text-xs font-black outline-none"><option value="domain">Domain</option><option value="ipcidr">IP CIDR</option><option value="classical">Classical</option></select></div>
               <Button onClick={() => void handleAdd()} disabled={isSaving || !newName.trim() || !newUrl.trim()} className="w-full h-11 bg-primary text-primary-foreground rounded-xl font-black text-[10px]">{isSaving ? <Loader2 className="size-4 animate-spin" /> : '确认新增规则集'}</Button>
            </div>
          )}
          <div className="space-y-3">
            {ruleSets.map((rs) => (
              <div key={rs.id} className="bg-muted/5 border-2 rounded-2xl p-4 group hover:border-primary/20 transition-all text-left text-foreground">
                <div className="flex justify-between items-start mb-3">
                   <div className="min-w-0 flex-1"><h5 className="text-xs font-black uppercase truncate">{rs.name}</h5><p className="text-[9px] font-mono text-muted-foreground truncate opacity-60">{rs.url}</p></div>
                   <div className="flex gap-1.5 opacity-0 group-hover:opacity-100 transition-all"><Button variant="ghost" size="icon" onClick={async () => { setSyncing(rs.id); await onRefresh(rs.id); setSyncing(null); }} className="size-7"><RefreshCw className={cn("size-3.5", syncing === rs.id && "animate-spin")} /></Button><Button variant="ghost" size="icon" onClick={() => onDelete(rs.id)} className="size-7 text-red-500"><Trash2 className="size-3.5" /></Button></div>
                </div>
                <div className="flex flex-wrap gap-x-4 gap-y-1 text-[9px] font-black uppercase text-muted-foreground"><span>Behavior: {rs.behavior || 'classical'}</span><span>Format: {rs.format || 'text'}</span><span>Entries: {rs.ruleCount || 0}</span><span>Update: {rs.lastUpdate}</span></div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
};

const RuleDrawer = ({ isOpen, onClose, onSave, proxies, nodes, ruleSets, initialData }: RuleDrawerProps) => {
  const [type, setType] = useState(initialData?.type || 'DOMAIN-SUFFIX');
  const [value, setValue] = useState(initialData?.value || '');
  const [policy, setPolicy] = useState(initialData?.policy || 'DIRECT');
  const [search, setSearch] = useState('');
  const { toast } = useToast();

  const targets = useMemo<RuleTarget[]>(() => {
    const list: RuleTarget[] = [
      { name: 'DIRECT', displayName: 'DIRECT', subscriptionName: null, type: 'system', icon: Zap },
      { name: 'REJECT', displayName: 'REJECT', subscriptionName: null, type: 'system', icon: ShieldAlert },
      ...proxies.map((proxy) => ({ name: proxy.name, displayName: displayRuntimeName(proxy.name, proxy.displayName), subscriptionName: proxy.subscriptionName, type: 'group' as const, icon: Layers })),
      ...nodes.map((node) => ({ name: node.name, displayName: displayRuntimeName(node.name, node.displayName), subscriptionName: node.subscriptionName, type: 'node' as const, icon: MousePointer2 }))
    ];
    const query = search.toLowerCase();
    return list.filter(target => [target.displayName, target.subscriptionName ?? ''].some(value => value.toLowerCase().includes(query)));
  }, [proxies, nodes, search]);

  const handleSave = () => {
    if (!value && type !== 'MATCH') return toast('参数缺失', 'error');
    onSave({ type, value: type === 'MATCH' ? 'ANY' : value, policy });
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-[110] flex justify-end overflow-hidden">
      <div className="absolute inset-0 bg-background/40 backdrop-blur-sm animate-in fade-in duration-500" onClick={onClose} />
      <div className="relative w-full max-w-md bg-card border-l h-full shadow-2xl flex flex-col animate-in slide-in-from-right duration-500 ease-out text-left text-foreground">
        <div className="p-6 border-b flex justify-between items-center bg-muted/5">
           <h3 className="text-lg font-black uppercase tracking-tight">{initialData ? '修改规则' : '新增规则'}</h3>
           <Button variant="ghost" size="icon" onClick={onClose} className="rounded-lg"><X className="size-5" /></Button>
        </div>
        <div className="flex-1 overflow-y-auto p-6 space-y-10 custom-scrollbar">
           <section className="space-y-4">
              <div className="flex items-center gap-2 text-muted-foreground text-[10px] font-black uppercase"><Fingerprint className="size-3" /> 01. 捕获</div>
              <div className="space-y-4">
                 <select value={type} onChange={e => { setType(e.target.value); setValue(''); }} className="w-full bg-background border-2 border-muted rounded-xl px-4 py-3.5 text-sm font-black outline-none focus:border-primary/40"><option value="RULE-SET">规则集</option><option value="DOMAIN-SUFFIX">域名后缀</option><option value="DOMAIN">精确域名</option><option value="IP-CIDR">IP 分段</option><option value="GEOIP">国家/地区</option><option value="MATCH">默认兜底</option></select>
                 {type === 'RULE-SET' ? (
                   <select value={value} onChange={e => setValue(e.target.value)} className="w-full bg-background border-2 border-muted rounded-xl px-4 py-3.5 text-sm font-black outline-none focus:border-primary/40">
                      <option value="">选择规则集...</option>{ruleSets.map((rs) => <option key={rs.id} value={rs.name}>{rs.name}</option>)}
                   </select>
                 ) : (
                   <input value={value} onChange={e => setValue(e.target.value)} disabled={type === 'MATCH'} placeholder="e.g. google.com" className="w-full bg-background border-2 border-muted rounded-xl px-4 py-3.5 text-sm font-black outline-none focus:border-primary/40" />
                 )}
              </div>
           </section>
           <section className="space-y-4">
              <div className="flex items-center gap-2 text-muted-foreground text-[10px] font-black uppercase"><ArrowDownRight className="size-3" /> 02. 指派</div>
              <div className="relative"><Search className="absolute left-4 top-1/2 -translate-y-1/2 size-4 text-muted-foreground opacity-70" /><input value={search} onChange={e => setSearch(e.target.value)} placeholder="检索出口引擎或物理节点..." className="w-full pl-11 pr-4 py-3.5 bg-background border rounded-xl text-xs font-black outline-none focus:border-primary/40 shadow-inner" /></div>
              <div className="grid grid-cols-1 gap-2 max-h-[300px] overflow-y-auto custom-scrollbar">
                  {targets.map(t => {
                    return (
                      <button key={t.name} onClick={() => setPolicy(t.name)} className={cn("flex items-center justify-between px-4 py-3 rounded-xl border transition-all", policy === t.name ? "bg-primary/10 border-primary" : "bg-muted/5 border-transparent hover:bg-muted/10")}>
                        <div className="flex items-center gap-3"><t.icon className="size-4 opacity-70" /><div className="flex items-center gap-2 text-xs font-black uppercase">{t.displayName} {t.subscriptionName && <SubBadge name={t.subscriptionName} />}</div></div>
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
  const [rules, setRules] = useState<Rule[]>([]);
  const [ruleSets, setRuleSets] = useState<RuleSet[]>([]);
  const [proxies, setProxies] = useState<ProxyGroup[]>([]);
  const [nodes, setNodes] = useState<ProxyNode[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState(false);
  const [search, setSearch] = useState('');
  const [isDrawerOpen, setIsDrawerOpen] = useState(false);
  const [isRSOpen, setIsRSOpen] = useState(false);
  const [isLabOpen, setIsLabOpen] = useState(false);
  const [editingRule, setEditingRule] = useState<Rule | null>(null);
  const [testTarget, setTarget] = useState('');
  const [testResult, setTestResult] = useState<RuleTestResult | null>(null);
  const [isTesting, setIsTesting] = useState(false);
  const [draggedRuleId, setDraggedRuleId] = useState<string | null>(null);
  const [movingRuleId, setMovingRuleId] = useState<string | null>(null);
  const displayNameByRuntimeName = useMemo(() => new Map<string, string>([
    ...proxies.map(proxy => [proxy.name, displayRuntimeName(proxy.name, proxy.displayName)] as const),
    ...nodes.map(node => [node.name, displayRuntimeName(node.name, node.displayName)] as const),
  ]), [proxies, nodes]);
  const subscriptionByPolicy = useMemo(() => new Map<string, string>([
    ...proxies.flatMap(proxy => proxy.subscriptionName ? [[proxy.name, proxy.subscriptionName] as const] : []),
    ...nodes.flatMap(node => node.subscriptionName ? [[node.name, node.subscriptionName] as const] : []),
  ]), [proxies, nodes]);

  const handleRetryLoad = async () => {
    setLoading(true);
    setLoadError(false);
    try {
      const [rData, pData, rsData] = await Promise.all([api.listRules(), api.proxyTopology(), api.listRuleSets()]);
      setRules(rData);
      setProxies(pData.groups || []);
      setNodes(pData.nodes || []);
      setRuleSets(rsData);
    } catch {
      setLoadError(true);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    let cancelled = false;
    void Promise.all([api.listRules(), api.proxyTopology(), api.listRuleSets()])
      .then(([rData, pData, rsData]) => {
        if (cancelled) return;
        setRules(rData);
        setProxies(pData.groups || []);
        setNodes(pData.nodes || []);
        setRuleSets(rsData);
      })
      .catch(() => {
        if (!cancelled) setLoadError(true);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const fetchRules = async () => { setRules(await api.listRules()); };
  const fetchRuleSets = async () => { setRuleSets(await api.listRuleSets()); };

  const handleSaveRule = async (data: RuleInput) => {
    try {
      if (editingRule) {
        await api.updateRule(editingRule.id, data);
      } else {
        await api.createRule(data);
      }
      toast('Success', 'success'); setIsDrawerOpen(false); setEditingRule(null); fetchRules();
    } catch { toast('Error', 'error'); }
  };

  const handleTestSandbox = async () => {
    if (!testTarget) return;
    setIsTesting(true);
    try {
      setTestResult(await api.testRule(testTarget));
    } catch {
      setTestResult(null);
      toast('当前规则类型无法在本地准确测试', 'error');
    } finally { setIsTesting(false); }
  };

  const handleDeleteRule = async (id: string) => {
    try {
      await api.deleteRule(id);
      await fetchRules();
      toast('规则已删除', 'success');
    } catch {
      toast('规则删除失败', 'error');
    }
  };

  const sourcePosition = (rule: Rule) => rules.filter(item => item.source === rule.source).findIndex(item => item.id === rule.id) + 1;
  const sourceRuleCount = (rule: Rule) => rules.filter(item => item.source === rule.source).length;
  const handleMoveRule = async (rule: Rule, position: number) => {
    if (movingRuleId) return;
    setMovingRuleId(rule.id);
    try {
      await api.updateRule(rule.id, {
        type: rule.type, value: rule.value, policy: rule.policy, desc: rule.desc,
        enabled: rule.enabled, position,
      });
      await fetchRules();
    } catch { toast('规则顺序调整失败', 'error'); }
    finally { setMovingRuleId(null); }
  };

  const handleDropRule = (target: Rule) => {
    const dragged = rules.find(rule => rule.id === draggedRuleId);
    setDraggedRuleId(null);
    if (!dragged || dragged.id === target.id || dragged.source !== target.source) return;
    void handleMoveRule(dragged, sourcePosition(target));
  };

  const handleAddRuleSet = async (data: RuleSetInput) => {
    try {
      await api.createRuleSet(data);
      await fetchRuleSets();
      toast('规则集已添加', 'success');
      return true;
    } catch {
      toast('规则集添加失败', 'error');
      return false;
    }
  };

  const handleRefreshRuleSet = async (id: string) => {
    try {
      await api.refreshRuleSet(id);
      await fetchRuleSets();
      toast('规则集已刷新', 'success');
    } catch {
      toast('规则集刷新失败', 'error');
    }
  };

  const handleDeleteRuleSet = async (id: string) => {
    try {
      await api.deleteRuleSet(id);
      await fetchRuleSets();
      toast('规则集已删除', 'success');
    } catch {
      toast('规则集删除失败', 'error');
    }
  };

  const filteredRules = useMemo(() => {
    return rules.filter(r => r.value.toLowerCase().includes(search.toLowerCase()) || r.policy.toLowerCase().includes(search.toLowerCase()) || r.type.toLowerCase().includes(search.toLowerCase()));
  }, [rules, search]);

  if (loading) return <div className="flex items-center justify-center h-[60vh]"><Loader2 className="animate-spin text-primary size-8" /></div>;

  if (loadError) {
    return (
      <div className="mx-auto flex min-h-[60vh] w-full max-w-2xl items-center px-4">
        <Alert variant="destructive">
          <ShieldAlert />
          <AlertTitle>规则数据加载失败</AlertTitle>
          <AlertDescription className="flex flex-col items-start gap-4">
            <p>无法读取规则、代理或规则集，请检查后端连接后重试。</p>
            <Button variant="outline" onClick={() => void handleRetryLoad()}>
              <RotateCcw data-icon="inline-start" />
              重试
            </Button>
          </AlertDescription>
        </Alert>
      </div>
    );
  }

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
      <div className="bg-card border border-border p-3 md:p-4 rounded-3xl shadow-sm flex flex-col md:flex-row items-center gap-4 text-left">
         <div className="relative flex-1 w-full">
            <Search className="absolute left-4 top-1/2 -translate-y-1/2 size-4 text-muted-foreground opacity-70" />
            <input value={search} onChange={e => setSearch(e.target.value)} placeholder="检索活跃分流记录..." className="w-full pl-11 pr-4 py-3 bg-background border border-muted rounded-2xl text-[11px] font-black uppercase outline-none focus:border-primary shadow-inner text-foreground" />
         </div>
         <div className="flex items-center gap-3 shrink-0 text-left">
            <Button onClick={() => setIsLabOpen(true)} variant="outline" className="h-11 px-6 rounded-xl font-black uppercase text-[9px] gap-2 bg-zinc-950 text-white hover:bg-black shadow-lg shadow-black/20"><FlaskConical className="size-4 text-blue-400" /> 追踪实验室</Button>
            <Button variant="outline" size="icon" onClick={() => { fetchRules(); fetchRuleSets(); }} className="size-11 rounded-xl border-2"><RotateCcw className="size-5" /></Button>
         </div>
      </div>

      {/* 3. The Table Header (Desktop Only) */}
      <div className={cn("hidden lg:grid gap-4 px-6 text-[10px] font-black uppercase text-muted-foreground tracking-wider select-none", GRID_COLS)}>
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
          <div key={rule.id} draggable={!movingRuleId} onDragStart={() => setDraggedRuleId(rule.id)} onDragEnd={() => setDraggedRuleId(null)} onDragOver={event => event.preventDefault()} onDrop={() => handleDropRule(rule)} className={cn("group relative bg-card hover:bg-card border border-border hover:border-primary/20 rounded-2xl p-3 md:px-6 md:py-4 transition-all duration-300 shadow-sm hover:shadow-md overflow-hidden text-left", draggedRuleId === rule.id && "opacity-50", movingRuleId === rule.id && "pointer-events-none opacity-60")}>
             {/* Desktop Layout */}
             <div className={cn("hidden lg:grid items-center gap-4", GRID_COLS)}>
                <div className="flex items-center justify-center gap-1">
                  <GripVertical className="size-3.5 cursor-grab text-muted-foreground" />
                  <select aria-label={`设置 ${rule.value} 的顺序`} value={sourcePosition(rule)} onChange={event => void handleMoveRule(rule, Number(event.target.value))} className="h-7 w-12 rounded-md border bg-background text-center font-mono text-[10px] font-black outline-none">
                    {Array.from({ length: sourceRuleCount(rule) }, (_, position) => <option key={position + 1} value={position + 1}>{position + 1}</option>)}
                  </select>
                </div>
                <div><div className="px-2 py-0.5 rounded bg-zinc-900 text-zinc-100 text-[9px] font-black uppercase tracking-wide inline-block border border-white/5">{rule.type}</div></div>
                <div className="min-w-0"><h4 className="text-sm font-black truncate text-foreground">{rule.value}</h4><p className="text-[10px] font-bold text-muted-foreground uppercase truncate">{rule.desc || 'Active Dispatching'}</p></div>
                <div className="flex justify-center"><LogicFlow policy={rule.policy} /></div>
                <div className="flex justify-start min-w-0"><ActionPill action={rule.policy} displayName={displayNameByRuntimeName.get(rule.policy)} subscriptionName={subscriptionByPolicy.get(rule.policy)} className="w-full max-w-[180px]" /></div>
                <div className="flex items-center justify-end gap-1.5 opacity-0 group-hover:opacity-100 transition-all pr-2">
                   <Button title="置顶" onClick={() => void handleMoveRule(rule, 1)} variant="ghost" size="icon" className="size-8 rounded-lg hover:bg-muted"><Pin className="size-3.5" /></Button>
                   <Button onClick={() => { setEditingRule(rule); setIsDrawerOpen(true); }} variant="ghost" size="icon" className="size-8 rounded-lg hover:bg-muted"><Edit3 className="size-3.5" /></Button>
                   <Button onClick={() => handleDeleteRule(rule.id)} variant="ghost" size="icon" className="size-8 rounded-lg text-red-500 hover:bg-red-500/10"><Trash2 className="size-3.5" /></Button>
                </div>
             </div>

             {/* Mobile Layout */}
             <div className="flex lg:hidden flex-col gap-4 text-left">
                <div className="flex items-start justify-between gap-4 text-left">
                   <div className="flex gap-3 min-w-0 text-left">
                      <span className="flex items-center gap-1 text-[10px] font-mono opacity-70 font-black pt-1"><GripVertical className="size-3.5" />{String(idx + 1).padStart(2, '0')}</span>
                      <div className="min-w-0 text-left"><div className="px-1.5 py-0.5 rounded bg-zinc-900 text-zinc-100 text-[9px] font-black uppercase tracking-wide mb-1.5 inline-block">{rule.type}</div><h4 className="text-sm font-black truncate text-foreground">{rule.value}</h4></div>
                   </div>
                   <div className="flex gap-1 shrink-0"><select aria-label="设置规则顺序" value={sourcePosition(rule)} onChange={event => void handleMoveRule(rule, Number(event.target.value))} className="h-8 w-12 rounded-lg border bg-background text-center text-[10px] font-black">{Array.from({ length: sourceRuleCount(rule) }, (_, position) => <option key={position + 1} value={position + 1}>{position + 1}</option>)}</select><Button title="置顶" onClick={() => void handleMoveRule(rule, 1)} variant="ghost" size="icon" className="size-8 rounded-lg bg-muted/50"><Pin className="size-3.5" /></Button><Button onClick={() => { setEditingRule(rule); setIsDrawerOpen(true); }} variant="ghost" size="icon" className="size-8 rounded-lg bg-muted/50"><Edit3 className="size-3.5" /></Button><Button onClick={() => handleDeleteRule(rule.id)} variant="ghost" size="icon" className="size-8 rounded-lg text-red-500 bg-red-500/5"><Trash2 className="size-3.5" /></Button></div>
                </div>
                <div className="flex items-center gap-3 pt-3 border-t border-dashed border-muted/50"><div className="flex-1 min-w-0"><ActionPill action={rule.policy} displayName={displayNameByRuntimeName.get(rule.policy)} subscriptionName={subscriptionByPolicy.get(rule.policy)} className="w-full" /></div></div>
             </div>

             {/* Dynamic Accent Accent - Fixed to clean thin line */}
             <div className={cn("absolute left-0 inset-y-3 w-0.5 rounded-r-full transition-all duration-500 opacity-60", rule.policy === 'DIRECT' ? "bg-green-500" : rule.policy === 'REJECT' ? "bg-rose-500" : "bg-primary")} />
          </div>
        ))}
        {filteredRules.length === 0 && <div className="py-32 border-4 border-dashed border-muted rounded-[3rem] flex flex-col items-center justify-center opacity-70 text-foreground text-left"><LayoutGrid className="size-16 mb-4" /><p className="text-xl font-black uppercase tracking-widest">Empty Logic Deck</p></div>}
      </div>

      {/* Lab Modal */}
      {isLabOpen && (
        <div className="fixed inset-0 z-[120] flex items-center justify-center p-4">
           <div className="absolute inset-0 bg-background/70" onClick={() => setIsLabOpen(false)} />
           <div className="relative w-full max-w-lg bg-zinc-950 text-white rounded-[3rem] p-8 md:p-12 shadow-2xl border-4 border-white/5 animate-in zoom-in-95 text-left">
              <div className="flex justify-between items-start mb-10 text-left">
                <div className="flex items-center gap-5 text-left">
                  <div className="size-16 rounded-[1.5rem] bg-blue-600 flex items-center justify-center shadow-2xl shadow-blue-600/40 shrink-0"><FlaskConical className="size-8" /></div>
                  <div className="text-left">
                    <h3 className="text-2xl font-black uppercase tracking-tight">追踪实验室</h3>
                    <p className="text-[10px] font-black text-blue-300 uppercase tracking-wider">Trace Sandbox</p>
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
                        { label: 'HIT RULE', display: `${testResult.hitRule.type}: ${testResult.hitRule.value}`, subscriptionName: null, icon: Shield, col: "text-zinc-500" },
                        { label: 'TARGET POLICY', display: displayNameByRuntimeName.get(testResult.hitRule.policy) ?? displayRuntimeName(testResult.hitRule.policy), subscriptionName: subscriptionByPolicy.get(testResult.hitRule.policy), icon: Layers, col: "text-primary" },
                        { label: 'EXIT NODE', display: displayNameByRuntimeName.get(testResult.finalProxy) ?? displayRuntimeName(testResult.finalProxy), subscriptionName: subscriptionByPolicy.get(testResult.finalProxy), icon: CheckCircle2, col: "text-green-500" }
                      ].map((step, i) => {
                         return (
                           <div key={i} className="flex items-center gap-6 text-left">
                              <div className="size-14 rounded-2xl bg-white/5 border border-white/10 flex items-center justify-center shrink-0"><step.icon className={cn("size-7", step.col)} /></div>
                              <div className="min-w-0 text-left">
                                 <p className="text-[9px] font-black text-zinc-500 uppercase tracking-widest mb-1.5">{step.label}</p>
                                 <div className="flex items-center gap-2">
                                    <p className={cn("text-xl font-black truncate", step.col)}>{step.display}</p>
                                    {step.subscriptionName && <SubBadge name={step.subscriptionName} />}
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

      {isDrawerOpen && <RuleDrawer isOpen={isDrawerOpen} onClose={() => setIsDrawerOpen(false)} onSave={handleSaveRule} proxies={proxies} nodes={nodes} ruleSets={ruleSets} initialData={editingRule} />}
      {isRSOpen && <RuleSetDrawer isOpen={isRSOpen} onClose={() => setIsRSOpen(false)} ruleSets={ruleSets} onRefresh={handleRefreshRuleSet} onDelete={handleDeleteRuleSet} onAdd={handleAddRuleSet} />}
    </div>
  );
};
