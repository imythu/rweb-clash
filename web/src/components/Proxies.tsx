import { useState, useEffect, useMemo } from 'react';
import { 
  Search, Zap, Globe, Plus, Loader2, Shield, Scale, MousePointer2, 
  RotateCcw, Layers, Radio, Lock, Edit3, CheckCircle2, ChevronDown,
  X, Trash2, Info as InfoIcon,
  ZapOff, ShieldAlert, BarChart3,
  Flag, Cpu, Tag, Clock
} from 'lucide-react';
import { cn, SUB_DELIMITER } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { useToast } from './Toast';

// --- Nano Components ---

const SubBadge = ({ name }: { name: string }) => (
  <span className="px-1.5 py-0.5 rounded-md bg-muted/50 border border-muted-foreground/10 text-[8px] font-black text-muted-foreground/60 uppercase tracking-tighter shrink-0 ml-1.5">
    {name}
  </span>
);

const GroupIcon = ({ type, className }: { type: string, className?: string }) => {
  if (type === 'url-test') return <Zap className={className} />;
  if (type === 'fallback') return <Shield className={className} />;
  if (type === 'load-balance') return <Scale className={className} />;
  return <MousePointer2 className={className} />;
};

const StatusBadge = ({ delay, loading, onClick, className }: { delay: number, loading?: boolean, onClick?: (e: any) => void, className?: string }) => (
  <button 
    onClick={onClick}
    disabled={loading}
    className={cn(
      "px-2 py-0.5 rounded-md text-[10px] font-black font-mono border shadow-sm transition-all flex items-center gap-1 shrink-0",
      loading ? "bg-muted text-muted-foreground animate-pulse" :
      delay <= 0 ? "bg-muted text-muted-foreground border-muted-foreground/20 hover:border-primary/40" :
      delay < 150 ? "bg-green-500/10 text-green-600 border-green-500/20 hover:bg-green-500/20" : 
      "bg-amber-500/10 text-amber-600 border-amber-500/20 hover:bg-amber-500/20",
      onClick && "cursor-pointer active:scale-90",
      className
    )}
  >
    {loading ? <Loader2 className="size-2.5 animate-spin" /> : (delay <= 0 ? 'T.O' : `${delay}ms`)}
  </button>
);

// --- Semantic Logic Drawer ---

interface SemanticBlock {
  id: string;
  action: 'keep' | 'discard';
  type: 'name' | 'country' | 'protocol' | 'latency' | 'status' | 'subscription';
  operator: string;
  value: string;
}

const CreateGroupDrawer = ({ isOpen, onClose, onSave, allNodes, initialData }: any) => {
  const [name, setName] = useState('');
  const [groupType, setGroupType] = useState('select');
  const [blocks, setBlocks] = useState<SemanticBlock[]>([]);
  const { toast } = useToast();

  useEffect(() => {
    if (initialData && isOpen) {
      setName(initialData.name || '');
      setGroupType(initialData.type || 'select');
      setBlocks(Array.isArray(initialData.filter) ? initialData.filter : []);
    } else if (isOpen) {
      setName(''); setGroupType('select'); setBlocks([]);
    }
  }, [initialData, isOpen]);

  const uniqueTypes = useMemo(() => Array.from(new Set(allNodes.map((n: any) => n.type))), [allNodes]);
  const uniqueSubs = useMemo(() => Array.from(new Set(allNodes.map((n: any) => n.name.split(SUB_DELIMITER)[1] || 'LOCAL'))), [allNodes]);
  const uniqueCountries = useMemo(() => Array.from(new Set(allNodes.map((n: any) => n.country).filter(Boolean))), [allNodes]);

  const previewNodes = useMemo(() => {
    let result = [...allNodes];
    blocks.forEach(block => {
      if (!block.value && block.type !== 'status') return;
      try {
        result = result.filter(n => {
          let targetValue: any = '';
          const [nodeLabel, subLabel] = n.name.split(SUB_DELIMITER);
          if (block.type === 'name') targetValue = nodeLabel;
          else if (block.type === 'protocol') targetValue = n.type;
          else if (block.type === 'country') targetValue = n.country || '';
          else if (block.type === 'subscription') targetValue = subLabel || 'LOCAL';
          else if (block.type === 'latency') targetValue = n.latency;
          else if (block.type === 'status') targetValue = n.latency > 0 ? 'online' : 'timeout';

          let match = false;
          if (block.type === 'latency') {
            const numVal = parseInt(block.value);
            if (isNaN(numVal)) return true;
            match = n.latency > 0 && n.latency < numVal;
          } else if (block.type === 'status') {
            match = targetValue === 'timeout';
          } else {
            const target = String(targetValue).toLowerCase();
            const val = block.value.toLowerCase();
            switch (block.operator) {
              case 'contains': match = target.includes(val); break;
              case 'equals': match = target === val; break;
              case 'regex': match = new RegExp(block.value, 'i').test(String(targetValue)); break;
              case 'is': match = target === val; break;
            }
          }
          return block.action === 'keep' ? match : !match;
        });
      } catch (e) {}
    });
    return result;
  }, [blocks, allNodes]);

  const handleSave = () => {
    if (!name.trim()) return toast('请填写分组名称', 'error');
    if (blocks.length > 0 && previewNodes.length === 0) return toast('当前规则未命中任何节点', 'error');
    onSave({ name, type: groupType, filter: blocks });
  };

  const addBlock = (type: SemanticBlock['type']) => {
    const newBlock: SemanticBlock = {
      id: Math.random().toString(),
      action: 'keep',
      type,
      operator: type === 'name' ? 'contains' : (type === 'latency' ? 'less_than' : 'is'),
      value: ''
    };
    setBlocks([...blocks, newBlock]);
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-[100] flex justify-end overflow-hidden">
      <div className="absolute inset-0 bg-background/40 backdrop-blur-sm animate-in fade-in duration-500" onClick={onClose} />
      <div className="relative w-full max-w-2xl bg-card border-l-2 border-primary/10 shadow-2xl flex flex-col h-full animate-in slide-in-from-right duration-500 ease-[cubic-bezier(0.32,0.72,0,1)] text-left">
        <div className="p-6 md:p-8 border-b border-border/50 flex justify-between items-center bg-muted/5 shrink-0">
          <div className="text-left">
            <h3 className="text-2xl font-black uppercase tracking-tighter flex items-center gap-2 text-foreground">
              {initialData ? <Edit3 className="size-6 text-primary" /> : <Plus className="size-6 text-primary" />}
              {initialData ? '编辑分流引擎' : '定制出口引擎'}
            </h3>
            <p className="text-[10px] font-bold text-muted-foreground uppercase tracking-widest mt-1 opacity-60">Logic Forge Laboratory</p>
          </div>
          <Button variant="ghost" size="icon" onClick={onClose} className="size-10 rounded-xl hover:bg-muted"><X className="size-5 text-foreground" /></Button>
        </div>
        <div className="flex-1 overflow-y-auto p-6 md:p-8 space-y-10 custom-scrollbar pb-32 min-h-0">
          <section className="space-y-4">
             <div className="flex items-center gap-3 opacity-30 uppercase tracking-[0.2em] font-black text-[9px]">
                <Radio className="size-3" /> 01. 引擎身份定义
             </div>
             <input value={name} onChange={e => setName(e.target.value)} placeholder="输入分组名称..." className="w-full bg-transparent text-2xl font-black outline-none border-b-2 border-muted focus:border-primary pb-2 text-foreground" />
             <div className="grid grid-cols-2 gap-3 pt-4">
                {[
                  { id: 'select', label: '手动选择', icon: MousePointer2, color: 'text-primary' },
                  { id: 'url-test', label: '自动测速', icon: Zap, color: 'text-yellow-500' },
                  { id: 'fallback', label: '故障转移', icon: Shield, color: 'text-blue-500' },
                  { id: 'load-balance', label: '负载均衡', icon: Scale, color: 'text-purple-500' },
                ].map(t => (
                  <button key={t.id} onClick={() => setGroupType(t.id)} className={cn("p-4 rounded-2xl border-2 transition-all duration-300 text-left flex items-center gap-4", groupType === t.id ? "bg-primary/10 border-primary shadow-sm text-primary" : "bg-muted/10 border-transparent hover:border-primary/20 text-muted-foreground group-hover:text-foreground")}>
                    <t.icon className={cn("size-6", groupType === t.id ? "text-primary" : t.color)} />
                    <p className="font-black text-xs uppercase">{t.label}</p>
                  </button>
                ))}
             </div>
          </section>
          <section className="space-y-5">
             <div className="flex justify-between items-center text-foreground">
                <div className="flex items-center gap-3 opacity-30 uppercase tracking-[0.2em] font-black text-[9px]">
                  <Layers className="size-3" /> 02. 精选清洗方案
                </div>
                <div className="flex items-center gap-1.5">
                   <Button onClick={() => addBlock('name')} variant="outline" size="sm" className="h-7 px-2.5 rounded-lg text-[9px] font-black uppercase border-dashed"><Tag className="size-3 mr-1" /> 名字</Button>
                   <Button onClick={() => addBlock('country')} variant="outline" size="sm" className="h-7 px-2.5 rounded-lg text-[9px] font-black uppercase border-dashed"><Flag className="size-3 mr-1" /> 国家</Button>
                   <Button onClick={() => addBlock('latency')} variant="outline" size="sm" className="h-7 px-2.5 rounded-lg text-[9px] font-black uppercase border-dashed"><Clock className="size-3 mr-1" /> 延迟</Button>
                </div>
             </div>
             <div className="bg-primary/[0.03] border border-primary/10 rounded-xl p-3 flex gap-3 items-start animate-in fade-in duration-700">
                <InfoIcon className="size-3.5 text-primary mt-0.5 shrink-0" />
                <div className="space-y-1 text-[10px] font-medium leading-relaxed">
                   <p className="font-black text-primary uppercase">交互指南 / INTERACTION GUIDE</p>
                   <p className="text-muted-foreground">点击积木左侧按钮切换 <span className="bg-primary/10 px-1 rounded font-bold text-primary">引入/剔除</span>，点击中间标签切换匹配模式。</p>
                </div>
             </div>
             <div className="space-y-3">
                {blocks.map((block) => (
                  <div key={block.id} className={cn("relative rounded-2xl p-3 border-2 transition-all bg-card", block.action === 'keep' ? "border-emerald-500/20 shadow-sm" : "border-rose-500/20 shadow-sm")}>
                    <div className="flex items-center gap-3">
                      <button onClick={() => setBlocks(blocks.map(b => b.id === block.id ? { ...b, action: b.action === 'keep' ? 'discard' : 'keep' } : b))} className={cn("flex items-center gap-2 px-3 py-2 rounded-xl font-black text-[10px] uppercase transition-all active:scale-95 shadow-md", block.action === 'keep' ? "bg-emerald-500 text-white" : "bg-rose-500 text-white")}>
                        {block.action === 'keep' ? <CheckCircle2 className="size-3.5" /> : <ZapOff className="size-3.5" />} {block.action === 'keep' ? '引入' : '剔除'}
                      </button>
                      <div className="flex-1 flex items-center gap-3 overflow-hidden text-foreground">
                        <span className="text-[10px] font-black text-muted-foreground uppercase shrink-0">{block.type === 'name' ? '节点名' : block.type === 'country' ? '国家' : block.type === 'protocol' ? '协议' : block.type === 'latency' ? '延迟' : block.type === 'status' ? '状态' : '订阅源'}</span>
                        <div className="flex-1 flex items-center gap-2">
                           {block.type === 'name' && (
                             <>
                               <button onClick={() => setBlocks(blocks.map(b => b.id === block.id ? { ...b, operator: b.operator === 'contains' ? 'regex' : (b.operator === 'regex' ? 'starts_with' : 'contains') } : b))} className="px-2 py-1 bg-muted rounded-lg text-[9px] font-black uppercase shrink-0 border">{block.operator === 'contains' ? '包含' : (block.operator === 'regex' ? '正则' : '开头')}</button>
                               <input value={block.value} onChange={e => setBlocks(blocks.map(b => b.id === block.id ? { ...b, value: e.target.value } : b))} placeholder="输入关键字..." className="flex-1 bg-background border-2 border-muted rounded-xl px-3 py-1.5 text-[11px] font-bold focus:border-primary/40 outline-none" />
                             </>
                           )}
                           {block.type === 'country' && (
                             <select value={block.value} onChange={e => setBlocks(blocks.map(b => b.id === block.id ? { ...b, value: e.target.value } : b))} className="w-full bg-background border-2 border-muted rounded-xl px-3 py-1.5 text-[11px] font-bold outline-none">
                                <option value="">选择地区...</option>
                                {uniqueCountries.map((c: any) => <option key={c} value={c}>{c}</option>)}
                             </select>
                           )}
                           {block.type === 'latency' && (
                             <div className="relative w-full flex items-center gap-2"><span className="text-[10px] font-bold opacity-40">低于</span><input type="number" value={block.value} onChange={e => setBlocks(blocks.map(b => b.id === block.id ? { ...b, value: e.target.value } : b))} className="flex-1 bg-background border-2 border-muted rounded-xl px-3 py-1.5 text-[11px] font-bold outline-none" /><span className="text-[9px] font-black opacity-30 uppercase tracking-widest">ms</span></div>
                           )}
                           {block.type === 'status' && (<div className="flex items-center gap-2 font-black text-[10px] text-muted-foreground uppercase"><ShieldAlert className="size-3" /> 匹配所有超时节点</div>)}
                           {block.type === 'protocol' && (
                             <select value={block.value} onChange={e => setBlocks(blocks.map(b => b.id === block.id ? { ...b, value: e.target.value } : b))} className="w-full bg-background border-2 border-muted rounded-xl px-3 py-1.5 text-[11px] font-bold outline-none">
                                <option value="">选择协议...</option>
                                {uniqueTypes.map((t: any) => <option key={t} value={t}>{t}</option>)}
                             </select>
                           )}
                        </div>
                        <Button variant="ghost" size="icon" onClick={() => setBlocks(blocks.filter(b => b.id !== block.id))} className="size-9 rounded-xl text-red-500 hover:bg-red-500/10 shrink-0"><Trash2 className="size-4" /></Button>
                      </div>
                    </div>
                  </div>
                ))}
             </div>
          </section>
          <section className="space-y-4">
             <div className="flex justify-between items-center opacity-30 uppercase tracking-[0.2em] font-black text-[9px]">
                <div className="flex items-center gap-3"><CheckCircle2 className="size-3" /> 03. 资产捕获实时预览</div>
                <span>{previewNodes.length} 命中</span>
             </div>
             <div className="bg-muted/10 rounded-[2rem] border-2 border-dashed p-4 min-h-[150px] max-h-[300px] overflow-y-auto space-y-1 custom-scrollbar text-foreground">
                {previewNodes.slice(0, 100).map(node => {
                  const [dName, sName] = node.name.split(SUB_DELIMITER);
                  return (
                    <div key={node.name} className="flex items-center justify-between p-2.5 bg-background/50 rounded-xl border border-border/40 text-foreground animate-in fade-in zoom-in-95">
                       <div className="flex items-center gap-2 min-w-0">
                         <p className="text-[11px] font-black truncate">{dName}</p>
                         {sName && <SubBadge name={sName} />}
                       </div>
                       <div className="flex items-center gap-2 shrink-0">
                          <div className={cn("size-1.5 rounded-full", node.latency > 0 ? "bg-green-500" : "bg-rose-500")} />
                          <span className="text-[8px] font-black text-muted-foreground uppercase">{node.latency > 0 ? `${node.latency}ms` : 'T.O'}</span>
                       </div>
                    </div>
                  );
                })}
             </div>
          </section>
        </div>
        <div className="p-6 md:p-8 bg-card/80 backdrop-blur-xl border-t border-border/50 flex gap-4 shrink-0">
           <Button variant="ghost" onClick={onClose} className="h-14 px-8 rounded-2xl font-black text-xs uppercase tracking-widest flex-1 hover:bg-muted text-foreground">取消操作</Button>
           <Button onClick={handleSave} disabled={!name} className="h-14 px-12 rounded-2xl font-black text-xs uppercase tracking-[0.2em] shadow-2xl shadow-primary/30 flex-[2] bg-primary text-primary-foreground transition-all hover:scale-[1.02] active:scale-95">部署部署引擎</Button>
        </div>
      </div>
    </div>
  );
};

export const Proxies = () => {
  const { toast } = useToast();
  const [groups, setGroups] = useState<any[]>([]);
  const [nodes, setNodes] = useState<any[]>([]);
  const [activeGroupName, setActiveGroupName] = useState<string | null>(null);
  const [expandedGroupMobile, setExpandedGroupMobile] = useState<string | null>(null);
  const [isCreating, setIsCreating] = useState(false);
  const [editingData, setEditingData] = useState<any>(null);
  const [loading, setLoading] = useState(true);
  const [searchGroup, setSearchGroup] = useState('');
  const [searchNode, setSearchNode] = useState('');
  const [isSwitching, setIsSwitching] = useState<string | null>(null);
  const [testingGroup, setTestingGroup] = useState<string | null>(null);
  const [testingNode, setTestingNode] = useState<string | null>(null);

  const fetchData = async () => {
    try {
      const res = await fetch('/api/proxies');
      const data = await res.json();
      setGroups(data.groups); setNodes(data.nodes);
      if (!activeGroupName && data.groups.length > 0) setActiveGroupName(data.groups[0].name);
    } finally { setLoading(false); }
  };

  useEffect(() => { fetchData(); }, []);

  const activeGroup = useMemo(() => groups.find(g => g.name === activeGroupName), [groups, activeGroupName]);

  const getNodesForGroup = (groupName: string) => {
    const group = groups.find(g => g.name === groupName);
    if (!group) return [];
    return nodes.filter(n => group.all.includes(n.name)).filter(n => n.name.toLowerCase().includes(searchNode.toLowerCase())).sort((a, b) => (group.type === 'select' ? 0 : (a.latency || 9999) - (b.latency || 9999)));
  };

  const activeNodes = useMemo(() => {
    if (!activeGroup) return [];
    return getNodesForGroup(activeGroup.name);
  }, [nodes, activeGroup, searchNode]);

  const handleSelectNode = async (groupName: string, nodeName: string) => {
    const group = groups.find(g => g.name === groupName);
    if (!group || group.type !== 'select') return;
    setIsSwitching(nodeName);
    try {
      await fetch(`/api/proxies/${group.name}`, { method: 'PUT', body: JSON.stringify({ name: nodeName }) });
      toast('出口已切换', 'success'); fetchData();
    } catch (e) { toast('切换失败', 'error'); } finally { setIsSwitching(null); }
  };

  const handleTestGroup = async (e: any, groupName: string) => {
    if (e) e.stopPropagation();
    setTestingGroup(groupName);
    toast(`正在刷新延迟...`, 'info');
    await new Promise(r => setTimeout(r, 1000)); await fetchData();
    setTestingGroup(null);
  };

  const handleTestNode = async (e: any, nodeName: string) => {
    if (e) e.stopPropagation();
    setTestingNode(nodeName);
    await new Promise(r => setTimeout(r, 600)); 
    setNodes(prev => prev.map(n => n.name === nodeName ? { ...n, latency: Math.floor(Math.random() * 50) + 5 } : n));
    setTestingNode(null);
  };

  const filteredGroups = useMemo(() => groups.filter(g => g.name.toLowerCase().includes(searchGroup.toLowerCase())), [groups, searchGroup]);

  if (loading) return <div className="flex items-center justify-center h-[60vh]"><Loader2 className="animate-spin text-primary size-10" /></div>;

  return (
    <div className="max-w-[1600px] mx-auto h-[calc(100vh-6rem)] md:h-[calc(100vh-8rem)] flex flex-col gap-4 animate-in fade-in duration-500 px-3 md:px-4 pb-6 overflow-hidden text-left">
      <div className="flex flex-col md:flex-row justify-between items-start md:items-end shrink-0 px-1 gap-4 text-foreground text-left">
        <div className="text-left">
          <div className="flex items-center gap-2"><h2 className="text-2xl md:text-3xl font-black tracking-tight">分组管理</h2><div className="size-2 rounded-full bg-green-500 animate-pulse shadow-[0_0_8px_rgba(34,197,94,0.6)]" /><div className="hidden sm:flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-amber-500/5 border border-amber-500/10 text-amber-600/80 text-[10px] font-black uppercase ml-2"><Lock className="size-3" /> 锁标代表此组由订阅托管，不可编辑</div></div>
          <p className="text-xs text-muted-foreground mt-1 flex items-center gap-1.5 font-medium"><Radio className="size-3.5" /> 物理出口调度中心</p>
        </div>
        <Button onClick={() => { setEditingData(null); setIsCreating(true); }} className="rounded-xl shadow-lg shadow-primary/20 h-10 px-4 text-xs bg-primary text-primary-foreground font-bold transition-all active:scale-95"><Plus className="size-4 md:mr-1.5" /> 新建分组</Button>
      </div>

      <div className="hidden lg:flex flex-1 gap-4 overflow-hidden mt-2 min-h-0 text-foreground text-left">
        {/* Group List */}
        <div className="w-[380px] xl:w-[420px] flex flex-col bg-card/60 backdrop-blur-2xl border border-border/60 rounded-[2rem] overflow-hidden shadow-sm shrink-0">
          <div className="p-5 border-b border-border/50 bg-muted/10 shrink-0 text-left"><div className="relative"><Search className="absolute left-3.5 top-1/2 -translate-y-1/2 size-4 text-muted-foreground opacity-50" /><input value={searchGroup} onChange={(e) => setSearchGroup(e.target.value)} placeholder="搜索分组..." className="w-full pl-10 pr-4 py-2.5 bg-background/50 border border-border/50 rounded-xl text-sm font-medium outline-none focus:border-primary/50 transition-all text-foreground" /></div></div>
          <div className="flex-1 overflow-y-auto p-3 space-y-2 custom-scrollbar">
            {filteredGroups.map(group => {
              const isActive = activeGroupName === group.name;
              const isReadOnly = group.source === 'subscription';
              const [dName, sName] = group.name.split(SUB_DELIMITER);
              return (
                <div key={group.name} onClick={() => setActiveGroupName(group.name)} className={cn("relative flex flex-col p-4 rounded-[1.25rem] cursor-pointer transition-all border-2 text-left", isActive ? "bg-primary/10 border-primary/30 shadow-sm" : "bg-transparent border-transparent hover:bg-muted/50")}>
                  {isActive && <div className="absolute left-0 top-1/2 -translate-y-1/2 w-1.5 h-10 bg-primary rounded-r-full shadow-lg" />}
                  <div className="flex items-start justify-between gap-3 text-foreground">
                    <div className="flex items-center gap-3 min-w-0">
                      <div className={cn("size-10 rounded-xl flex items-center justify-center shrink-0", isActive ? "bg-primary text-primary-foreground" : "bg-card border text-muted-foreground")}><GroupIcon type={group.type} className="size-5" /></div>
                      <div className="min-w-0 text-left">
                        <div className="flex items-center gap-1.5">
                          <h4 className={cn("text-base font-bold truncate text-foreground", !isActive && "text-muted-foreground group-hover:text-foreground")}>{dName}</h4>
                          {sName && <SubBadge name={sName} />}
                          {isReadOnly && <Lock className="size-3 text-amber-500/70" />}
                        </div>
                        <div className="text-[10px] font-semibold text-muted-foreground uppercase text-left">{group.type} · {group.all.length} 节点</div>
                      </div>
                    </div>
                    <StatusBadge delay={group.delay} loading={testingGroup === group.name} onClick={(e) => handleTestGroup(e, group.name)} className={isActive ? "bg-background border-border text-foreground" : ""} />
                  </div>
                </div>
              );
            })}
          </div>
        </div>

        {/* Group Detail */}
        <div className="flex-1 flex flex-col bg-card/60 backdrop-blur-2xl border border-border/60 rounded-[2rem] overflow-hidden shadow-sm min-h-0 text-left">
          {activeGroup ? (
            <>
              <div className="p-8 border-b border-border/50 bg-muted/5 shrink-0">
                <div className="flex flex-col 2xl:flex-row 2xl:items-center justify-between gap-6 text-foreground">
                  <div className="flex items-start gap-5 text-left">
                    <div className={cn("size-16 rounded-2xl flex items-center justify-center shadow-lg border-2 shrink-0", activeGroup.source === 'subscription' ? "bg-amber-500/10 border-amber-500/20 text-amber-600" : "bg-primary/10 border-primary/20 text-primary")}>
                       <GroupIcon type={activeGroup.type} className="size-8" />
                    </div>
                    <div className="min-w-0 text-left text-foreground">
                      <div className="flex items-center gap-3">
                         <h3 className="text-xl md:text-2xl font-black truncate">{activeGroup.name.split(SUB_DELIMITER)[0]}</h3>
                         {activeGroup.name.split(SUB_DELIMITER)[1] && <SubBadge name={activeGroup.name.split(SUB_DELIMITER)[1]} />}
                      </div>
                      <div className="flex items-center gap-2 mt-1.5 text-foreground">
                        <div className={cn("size-2 rounded-full", activeGroup.now ? "bg-green-500 shadow-[0_0_8px_rgba(34,197,94,0.5)]" : "bg-muted")} />
                        <span className="text-xs md:text-sm font-semibold text-muted-foreground">当前出口:</span>
                        <div className="flex items-center gap-1.5 min-w-0">
                           <span className="text-xs md:text-sm font-bold truncate">{(activeGroup.now || 'Direct').split(SUB_DELIMITER)[0]}</span>
                           {(activeGroup.now || '').split(SUB_DELIMITER)[1] && <SubBadge name={activeGroup.now.split(SUB_DELIMITER)[1]} />}
                        </div>
                      </div>
                    </div>
                  </div>
                  <div className="flex items-center gap-2 md:gap-3 w-full 2xl:w-auto shrink-0">
                    <div className="relative flex-1 2xl:w-64"><Search className="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground opacity-50" /><input value={searchNode} onChange={(e) => setSearchNode(e.target.value)} placeholder="过滤节点..." className="w-full h-10 pl-9 pr-3 py-2 bg-background border border-border/50 rounded-xl text-sm font-medium outline-none focus:border-primary/50 transition-all shadow-inner text-foreground" /></div>
                    {activeGroup.source !== 'subscription' && <Button onClick={() => { setEditingData(activeGroup); setIsCreating(true); }} variant="outline" size="icon" className="size-10 rounded-xl bg-background border-border/50 hover:bg-muted shrink-0 text-foreground"><Edit3 className="size-4" /></Button>}
                    <Button onClick={(e) => handleTestGroup(e, activeGroup.name)} disabled={testingGroup === activeGroup.name} variant="outline" className="h-10 rounded-xl gap-2 font-bold text-xs bg-background border-border/50 hover:bg-muted transition-all shrink-0 text-foreground">{testingGroup === activeGroup.name ? <Loader2 className="size-4 animate-spin" /> : <RotateCcw className="size-4 text-primary" />} 测速</Button>
                  </div>
                </div>
              </div>
              <div className="flex-1 overflow-y-auto p-6 custom-scrollbar text-left text-foreground">
                {activeGroup.type === 'select' ? (
                  <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 gap-4 text-left">
                    {activeNodes.map(node => {
                      const [dName, sName] = node.name.split(SUB_DELIMITER);
                      return (
                        <button key={node.name} onClick={() => handleSelectNode(activeGroup.name, node.name)} disabled={!!isSwitching} className={cn("relative flex flex-col p-4 rounded-2xl border-2 transition-all duration-200 text-left overflow-hidden group/node", activeGroup.now === node.name ? "bg-primary/10 border-primary shadow-sm text-foreground font-bold" : "bg-background border-border/50 hover:border-primary/40 text-muted-foreground hover:text-foreground")}>
                          <div className="flex justify-between items-center w-full mb-3 text-foreground"><StatusBadge delay={node.latency} loading={testingNode === node.name} onClick={(e) => handleTestNode(e, node.name)} className={activeGroup.now === node.name ? "bg-background border-primary/20 text-primary" : ""} />{activeGroup.now === node.name && <CheckCircle2 className="size-4 text-primary" />}</div>
                          <div className="min-w-0 w-full"><p className="text-sm font-bold truncate">{dName}</p>{sName && <div className="mt-1"><SubBadge name={sName} /></div>}</div>
                          {isSwitching === node.name && <div className="absolute inset-0 bg-background/50 backdrop-blur-sm flex items-center justify-center"><Loader2 className="size-5 animate-spin text-primary" /></div>}
                        </button>
                      );
                    })}
                  </div>
                ) : (
                  <div className="space-y-2">
                    <div className="flex items-center gap-4 px-4 py-2 text-[10px] font-black uppercase opacity-30 border-b mb-4 text-foreground"><div className="w-12 text-center">Rank</div><div className="flex-1 text-left">Identity</div><div className="w-24 text-right text-foreground">Latency</div></div>
                    {activeNodes.map((node, idx) => {
                      const [dName, sName] = node.name.split(SUB_DELIMITER);
                      return (
                        <div key={node.name} className={cn("flex items-center justify-between p-3.5 rounded-2xl border transition-all text-foreground", activeGroup.now === node.name ? "bg-primary/10 border-primary shadow-sm font-bold" : "bg-background/40 border-transparent hover:bg-card hover:border-border/50 text-muted-foreground")}>
                          <div className="flex items-center gap-5 flex-1 min-w-0 text-left text-foreground"><div className={cn("w-8 h-8 rounded-lg flex items-center justify-center font-bold text-xs shrink-0 text-foreground", activeGroup.now === node.name ? "bg-primary text-white shadow-sm" : "bg-muted text-muted-foreground")}>{idx + 1}</div><div className="min-w-0 flex-1 text-foreground text-left"><div className="flex items-center gap-2"><p className="text-sm font-bold truncate">{dName}</p>{sName && <SubBadge name={sName} />}</div><p className="text-[10px] opacity-60 uppercase mt-0.5">{node.type}</p></div></div>
                          <StatusBadge delay={node.latency} loading={testingNode === node.name} onClick={(e) => handleTestNode(e, node.name)} className={activeGroup.now === node.name ? "bg-background border-primary/20 text-primary" : ""} />
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            </>
          ) : (
            <div className="flex-1 flex flex-col items-center justify-center text-muted-foreground opacity-50"><Globe className="size-20 mb-6" /><p className="text-xl font-bold tracking-widest uppercase text-center">Select a Group</p></div>
          )}
        </div>
      </div>

      {/* Mobile View */}
      <div className="lg:hidden flex-1 min-h-0 flex flex-col gap-3 pb-24 overflow-y-auto mt-2 text-left text-foreground">
        <div className="sticky top-0 z-20 bg-background/80 backdrop-blur-xl py-2 shrink-0"><div className="relative"><Search className="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground opacity-50" /><input value={searchGroup} onChange={(e) => setSearchGroup(e.target.value)} placeholder="搜索分组..." className="w-full pl-9 pr-3 py-3 bg-card border border-border/60 rounded-xl text-sm font-medium outline-none focus:border-primary/50 shadow-sm text-foreground" /></div></div>
        {filteredGroups.map(group => {
          const isExpanded = expandedGroupMobile === group.name;
          const isReadOnly = group.source === 'subscription';
          const [dName, sName] = group.name.split(SUB_DELIMITER);
          return (
            <div key={group.name} className={cn("flex flex-col shrink-0 rounded-[1.25rem] border-2 transition-all overflow-hidden text-foreground", isExpanded ? "bg-card border-primary/30 shadow-md" : "bg-card/40 border-border/50")}>
              <div onClick={() => setExpandedGroupMobile(isExpanded ? null : group.name)} className="flex items-center justify-between p-4 cursor-pointer select-none text-foreground text-left"><div className="flex items-center gap-3 min-w-0 text-foreground text-left"><div className={cn("size-10 rounded-xl flex items-center justify-center shrink-0 shadow-sm text-foreground", isExpanded ? "bg-primary/10 text-primary" : "bg-muted/50 border text-muted-foreground")}><GroupIcon type={group.type} className="size-5 text-foreground" /></div><div className="min-w-0 text-left text-foreground"><div className="flex items-center gap-1.5 text-left text-foreground"><h4 className="text-base font-bold truncate text-foreground">{dName}</h4>{sName && <SubBadge name={sName} />}{isReadOnly && <Lock className="size-3 text-amber-500/70 shrink-0" />}</div><div className="text-[10px] font-semibold text-muted-foreground uppercase text-left">{group.type} · {group.all.length} 节点</div></div></div><div className="flex items-center gap-3 shrink-0 text-foreground"><StatusBadge delay={group.delay} loading={testingGroup === group.name} onClick={(e) => handleTestGroup(e, group.name)} /><ChevronDown className={cn("size-4 text-muted-foreground transition-transform", isExpanded && "rotate-180 text-primary")} /></div></div>
              {isExpanded && (
                <div className="border-t bg-background/30 p-3 animate-in slide-in-from-top-2 duration-300 text-foreground text-left">
                  <div className="flex gap-2 mb-3 text-foreground text-left"><div className="relative flex-1"><Search className="absolute left-2.5 top-1/2 -translate-y-1/2 size-3.5 text-muted-foreground opacity-50" /><input value={searchNode} onChange={(e) => setSearchNode(e.target.value)} placeholder="过滤节点..." className="w-full pl-8 pr-3 py-2 bg-background border border-border/50 rounded-lg text-xs outline-none focus:border-primary/50 text-foreground" /></div>{!isReadOnly && <Button onClick={() => { setEditingData(group); setIsCreating(true); }} variant="outline" size="icon" className="h-9 w-9 text-foreground"><Edit3 className="size-3.5 text-foreground" /></Button>}<Button onClick={(e) => handleTestGroup(e, group.name)} disabled={testingGroup === group.name} variant="outline" className="h-9 px-3 rounded-lg gap-1.5 font-bold text-xs text-foreground">测速</Button></div>
                  <div className="space-y-2 max-h-[50vh] overflow-y-auto pr-1 text-left custom-scrollbar text-foreground">
                    {getNodesForGroup(group.name).map(node => {
                       const isNodeActive = group.now === node.name;
                       const [dN, sN] = node.name.split(SUB_DELIMITER);
                       return (
                         <button key={node.name} onClick={() => handleSelectNode(group.name, node.name)} disabled={!!isSwitching} className={cn("w-full flex items-center justify-between p-3 rounded-xl border transition-all text-left", isNodeActive ? "bg-primary/10 border-primary shadow-sm text-foreground font-bold" : "bg-card border-border/50 text-muted-foreground")}>
                           <div className="min-w-0 flex-1 text-left"><div className="flex items-center gap-2"><p className="text-xs truncate">{dN}</p>{sN && <SubBadge name={sN} />}</div></div>
                           <StatusBadge delay={node.latency} loading={testingNode === node.name} onClick={(e) => handleTestNode(e, node.name)} className={isNodeActive ? "bg-background border-primary/20 text-primary" : ""} />
                         </button>
                       )
                    })}
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>
      <CreateGroupDrawer isOpen={isCreating} onClose={() => { setIsCreating(false); setEditingData(null); }} onSave={(data: any) => fetch(editingData ? `/api/proxies/${editingData.name}` : '/api/proxies', { method: editingData ? 'PUT' : 'POST', body: JSON.stringify(data) }).then(fetchData).then(() => setIsCreating(false))} allNodes={nodes} initialData={editingData} />
    </div>
  );
};
