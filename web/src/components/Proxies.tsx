import { useState, useEffect, useMemo, useRef, useCallback, type MouseEvent as ReactMouseEvent } from 'react';
import { 
  Search, Zap, Globe, Plus, Loader2, Shield, Scale, MousePointer2, 
  RotateCcw, Layers, Radio, Lock, Edit3, CheckCircle2, ChevronDown,
  X, Trash2, Info as InfoIcon,
  ZapOff, ShieldAlert,
  Flag, Tag, Clock
} from 'lucide-react';
import { cn, createId, SUB_DELIMITER } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { useToast } from './toast-context';
import { api, type GroupFilter, type ProxyGroup, type ProxyGroupInput, type ProxyNode } from '@/lib/api';

// --- Nano Components ---

const SubBadge = ({ name }: { name: string }) => (
  <span title={name} className="ml-1.5 max-w-28 shrink-0 truncate rounded-md border border-muted-foreground/10 bg-muted px-1.5 py-0.5 text-[9px] font-black uppercase text-muted-foreground">
    {name}
  </span>
);

const GroupIcon = ({ type, className }: { type: string, className?: string }) => {
  if (type === 'url-test') return <Zap className={className} />;
  if (type === 'fallback') return <Shield className={className} />;
  if (type === 'load-balance') return <Scale className={className} />;
  return <MousePointer2 className={className} />;
};

const StatusBadge = ({ delay, loading, onClick, className }: { delay: number, loading?: boolean, onClick?: (event: ReactMouseEvent<HTMLButtonElement>) => void, className?: string }) => (
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

const isSystemBuiltinGroup = (group: Pick<ProxyGroup, 'builtin' | 'source'>) =>
  Boolean(group.builtin) || group.source === 'system';

const isManagedGroup = (group: Pick<ProxyGroup, 'builtin' | 'source'>) =>
  isSystemBuiltinGroup(group) || group.source === 'subscription';

const displayRuntimeName = (name: string, displayName?: string | null) =>
  displayName || name.split(SUB_DELIMITER)[0] || name;

const COUNTRY_PRESET_PREFIX = '国家 · ';
const COUNTRY_PRESET_DISABLED_KEY = 'rweb-clash.country-presets-disabled';
const GROUP_STRATEGIES = ['url-test', 'select', 'fallback', 'load-balance'] as const;
const GROUP_STRATEGY_LABELS: Record<string, string> = {
  'url-test': '自动测速',
  select: '手动选择',
  fallback: '故障转移',
  'load-balance': '负载均衡',
};

const presetCountry = (group: ProxyGroup) => {
  if (group.source !== 'custom' || !group.name.startsWith(COUNTRY_PRESET_PREFIX) || group.filter.length !== 1) return null;
  const filter = group.filter[0];
  if (filter.action !== 'keep' || filter.type !== 'country') return null;
  return filter.values?.length === 1 ? filter.values[0] : filter.value || null;
};

type SelectOption = {
  value: string;
  label: string;
  meta?: string;
};

const StyledSelect = ({
  value = '',
  values = [],
  placeholder,
  options,
  onChange,
  className,
  multiple = false,
}: {
  value?: string;
  values?: string[];
  placeholder: string;
  options: SelectOption[];
  onChange: (value: string | string[]) => void;
  className?: string;
  multiple?: boolean;
}) => {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const selectedValues = values.map(item => item.trim()).filter(Boolean);
  const selectedSet = new Set(selectedValues);
  const selected = options.find(option => option.value === value);
  const selectedOptions = options.filter(option => selectedSet.has(option.value));
  const displayText = multiple
    ? selectedOptions.length === 0
      ? placeholder
      : selectedOptions.length === 1
        ? selectedOptions[0].label
        : `${selectedOptions.length} 项已选`
    : selected?.label ?? placeholder;

  const toggleOption = (optionValue: string) => {
    if (!multiple) {
      onChange(optionValue);
      setOpen(false);
      return;
    }
    const nextValues = selectedSet.has(optionValue)
      ? selectedValues.filter(item => item !== optionValue)
      : [...selectedValues, optionValue];
    onChange(nextValues);
  };

  useEffect(() => {
    if (!open) return;
    const handlePointerDown = (event: PointerEvent) => {
      if (!ref.current?.contains(event.target as Node)) setOpen(false);
    };
    window.addEventListener('pointerdown', handlePointerDown);
    return () => window.removeEventListener('pointerdown', handlePointerDown);
  }, [open]);

  return (
    <div ref={ref} className={cn("relative min-w-0", className)}>
      <button
        type="button"
        onClick={() => setOpen(!open)}
        className={cn(
          "w-full h-8 bg-background border-2 border-muted rounded-xl px-3 text-left text-[11px] font-bold outline-none transition-all flex items-center justify-between gap-2 shadow-inner",
          open ? "border-primary/40 ring-2 ring-primary/10" : "hover:border-primary/30"
        )}
      >
        <span className={cn("truncate", (multiple ? selectedOptions.length === 0 : !selected) && "text-muted-foreground/60")}>{displayText}</span>
        <ChevronDown className={cn("size-3.5 shrink-0 text-muted-foreground transition-transform", open && "rotate-180 text-primary")} />
      </button>
      {open && (
        <div className="absolute left-0 right-0 top-[calc(100%+0.35rem)] z-[130] max-h-64 overflow-y-auto rounded-2xl border-2 border-border/70 bg-card p-1.5 shadow-2xl shadow-black/10 custom-scrollbar animate-in fade-in zoom-in-95 duration-150">
          {multiple && selectedOptions.length > 0 && (
            <div className="flex items-center justify-between gap-2 border-b border-border/60 px-2 pb-1.5 mb-1">
              <span className="text-[9px] font-black text-muted-foreground uppercase">{selectedOptions.length} selected</span>
              <button type="button" onClick={() => onChange([])} className="px-2 py-1 rounded-lg text-[9px] font-black text-red-500 hover:bg-red-500/10">清空</button>
            </div>
          )}
          {options.length === 0 ? (
            <div className="px-3 py-2 text-[10px] font-black text-muted-foreground/60">暂无选项</div>
          ) : options.map(option => (
            <button
              type="button"
              key={option.value}
              onClick={() => toggleOption(option.value)}
              className={cn(
                "w-full rounded-xl px-3 py-2 text-left transition-all flex items-center justify-between gap-3",
                (multiple ? selectedSet.has(option.value) : value === option.value) ? "bg-primary/10 text-primary" : "hover:bg-muted/60 text-foreground"
              )}
            >
              <span className="min-w-0">
                <span className="block truncate text-[11px] font-black">{option.label}</span>
                {option.meta && <span className="block truncate text-[9px] font-mono text-muted-foreground/70 mt-0.5">{option.meta}</span>}
              </span>
              {(multiple ? selectedSet.has(option.value) : value === option.value) && <CheckCircle2 className="size-3.5 shrink-0" />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
};

// --- Semantic Logic Drawer ---

interface SemanticBlock {
  id: string;
  action: 'keep' | 'discard';
  type: 'name' | 'country' | 'protocol' | 'latency' | 'status' | 'subscription';
  operator: string;
  value: string;
  values: string[];
  enabled: boolean;
}

type NormalizableBlock = Partial<GroupFilter> & { field?: string };

const normalizeBlock = (block: NormalizableBlock): SemanticBlock => {
  const type = block.type || block.field || 'name';
  const rawOperator = block.operator || (type === 'latency' ? 'less_than' : (type === 'country' ? 'in' : 'contains'));
  const rawValue = String(block.value || '').trim();
  const rawValues = Array.isArray(block.values) ? block.values.map(String).map(value => value.trim()).filter(Boolean) : [];
  const values = rawValues.length > 0
    ? rawValues
    : rawOperator === 'in'
      ? rawValue.split(',').map(item => item.trim()).filter(Boolean)
      : rawOperator === 'equals' && type === 'name' && rawValue
        ? [rawValue]
        : type === 'country' && rawValue
          ? [rawValue]
          : [];
  const operator = values.length > 0 && (type === 'country' || rawOperator === 'equals' || rawOperator === 'in') ? 'in' : rawOperator;

  return {
    id: block.id || createId(),
    action: (block.action || 'keep') as SemanticBlock['action'],
    type: type as SemanticBlock['type'],
    operator,
    value: operator === 'in' ? '' : rawValue,
    values,
    enabled: block.enabled !== false,
  };
};

const hasBlockValue = (block: SemanticBlock) =>
  block.type === 'status' || Boolean(block.value.trim()) || block.values.some(value => Boolean(value.trim()));

const shouldPersistBlock = (block: SemanticBlock) => !block.enabled || hasBlockValue(block);

interface CreateGroupDrawerProps {
  isOpen: boolean;
  onClose: () => void;
  onSave: (data: ProxyGroupInput) => Promise<boolean>;
  allNodes: ProxyNode[];
  initialData: ProxyGroup | null;
}

const CreateGroupDrawer = ({ isOpen, onClose, onSave, allNodes, initialData }: CreateGroupDrawerProps) => {
  const [name, setName] = useState(() => initialData?.name ?? '');
  const [groupType, setGroupType] = useState(() => initialData?.type ?? 'select');
  const [blocks, setBlocks] = useState<SemanticBlock[]>(() => initialData?.filter.map(normalizeBlock) ?? []);
  const [isSaving, setIsSaving] = useState(false);
  const saveInFlight = useRef(false);
  const { toast } = useToast();

  const uniqueTypes = useMemo(() => Array.from(new Set(allNodes.map(node => node.type))), [allNodes]);
  const uniqueCountries = useMemo(() => Array.from(new Set(allNodes.map(node => node.country).filter((country): country is string => Boolean(country)))), [allNodes]);

  const nameOperators = [
    { value: 'contains', label: '包含' },
    { value: 'in', label: '等于' },
    { value: 'regex', label: '正则' },
    { value: 'starts_with', label: '开头' },
  ];

  const nextNameOperator = (operator: string) => {
    const normalized = operator === 'equals' ? 'in' : operator;
    const index = nameOperators.findIndex(op => op.value === normalized);
    return nameOperators[(index + 1) % nameOperators.length].value;
  };

  const nameOperatorLabel = (operator: string) => {
    if (operator === 'equals') return '等于';
    return nameOperators.find(op => op.value === operator)?.label ?? '包含';
  };

  const nodeOptionLabel = (node: ProxyNode) => {
    const nodeLabel = displayRuntimeName(node.name, node.displayName);
    const subLabel = node.subscriptionName;
    return subLabel ? `${nodeLabel} / ${subLabel}` : nodeLabel;
  };

  const nodeOptions = useMemo<SelectOption[]>(() => allNodes.map((node: ProxyNode) => ({
    value: node.name,
    label: nodeOptionLabel(node),
    meta: `${node.country || 'UNK'} · ${node.type}`,
  })), [allNodes]);
  const countryOptions = useMemo<SelectOption[]>(() => uniqueCountries.map(country => ({
    value: country,
    label: country,
  })), [uniqueCountries]);
  const protocolOptions = useMemo<SelectOption[]>(() => uniqueTypes.map(type => ({
    value: type,
    label: type,
  })), [uniqueTypes]);

  const previewNodes = useMemo(() => {
    const activeBlocks = blocks.filter(block => block.enabled && hasBlockValue(block));
    const hasKeep = activeBlocks.some(block => block.action === 'keep');
    return allNodes.filter(n => {
      let included = !hasKeep;
      activeBlocks.forEach(block => {
      const blockValues = block.values ?? [];
      const blockValue = block.value.trim();
      let expression: RegExp | null = null;
      if (block.operator === 'regex') {
        try {
          expression = new RegExp(blockValue);
        } catch {
          return;
        }
      }
        let targetValue: string | number = '';
        const nodeLabel = displayRuntimeName(n.name, n.displayName);
        if (block.type === 'name') targetValue = block.operator === 'equals' || block.operator === 'in' ? n.name : nodeLabel;
        else if (block.type === 'protocol') targetValue = n.type;
        else if (block.type === 'country') targetValue = n.country || '';
        else if (block.type === 'subscription') targetValue = n.subscriptionName || 'LOCAL';
        else if (block.type === 'latency') targetValue = n.latency;
        else if (block.type === 'status') targetValue = n.latency > 0 ? 'online' : 'timeout';

        let matched = false;
        if (block.type === 'latency') {
          const threshold = Number.parseInt(blockValue, 10);
          matched = threshold === -1 ? n.latency <= 0 : n.latency > 0 && n.latency < threshold;
        } else {
          const target = String(targetValue).toLowerCase();
          const value = (block.type === 'status' && !blockValue ? 'timeout' : blockValue).toLowerCase();
          switch (block.operator) {
            case 'contains': matched = target.includes(value); break;
            case 'equals':
            case 'is': matched = target === value; break;
            case 'in': matched = blockValues.map(item => item.trim().toLowerCase()).filter(Boolean).includes(target); break;
            case 'regex': matched = expression?.test(String(targetValue)) ?? false; break;
            case 'starts_with': matched = target.startsWith(value); break;
          }
        }
        if (matched) included = block.action === 'keep';
      });
      return included;
    });
  }, [blocks, allNodes]);

  const handleSave = async () => {
    if (saveInFlight.current) return;
    if (!name.trim()) return toast('请填写分组名称', 'error');
    if (blocks.length > 0 && previewNodes.length === 0) return toast('当前规则未命中任何节点', 'error');
    const filter = blocks.filter(shouldPersistBlock).map(block => {
      const base = {
        id: block.id,
        action: block.action,
        type: block.type,
        operator: block.operator,
        enabled: block.enabled,
      };
      return block.operator === 'in'
        ? { ...base, values: block.values.map(value => value.trim()).filter(Boolean) }
        : { ...base, value: block.value.trim() };
    });
    saveInFlight.current = true;
    setIsSaving(true);
    try {
      await onSave({ name: name.trim(), type: groupType, filter });
    } finally {
      saveInFlight.current = false;
      setIsSaving(false);
    }
  };

  const addBlock = (type: SemanticBlock['type']) => {
    const newBlock: SemanticBlock = {
      id: createId(),
      action: 'keep',
      type,
      operator: type === 'name' ? 'contains' : (type === 'latency' ? 'less_than' : (type === 'country' ? 'in' : 'is')),
      value: type === 'status' ? 'timeout' : '',
      values: [],
      enabled: true,
    };
    setBlocks([...blocks, newBlock]);
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-[100] flex justify-end overflow-hidden">
      <div className="absolute inset-0 bg-background/40 backdrop-blur-sm animate-in fade-in duration-500" onClick={onClose} />
      <div className="relative w-full max-w-2xl bg-card border-l-2 border-primary/10 shadow-2xl flex flex-col h-full animate-in slide-in-from-right duration-500 ease-out text-left">
        <div className="p-6 md:p-8 border-b border-border/50 flex justify-between items-center bg-muted/5 shrink-0">
          <div className="text-left">
            <h3 className="text-2xl font-black uppercase tracking-tighter flex items-center gap-2 text-foreground">
              {initialData ? <Edit3 className="size-6 text-primary" /> : <Plus className="size-6 text-primary" />}
              {initialData ? '编辑分流引擎' : '定制出口引擎'}
            </h3>
            <p className="text-[10px] font-bold text-muted-foreground uppercase tracking-widest mt-1">Logic Forge Laboratory</p>
          </div>
          <Button variant="ghost" size="icon" onClick={onClose} className="size-10 rounded-xl hover:bg-muted"><X className="size-5 text-foreground" /></Button>
        </div>
        <div className="flex-1 overflow-y-auto p-6 md:p-8 space-y-10 custom-scrollbar pb-32 min-h-0">
          <section className="space-y-4">
             <div className="flex items-center gap-3 text-muted-foreground uppercase tracking-wider font-black text-[10px]">
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
                <div className="flex items-center gap-3 uppercase tracking-[0.16em] font-black text-[10px] text-muted-foreground">
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
                      <div className="flex-1 flex items-center gap-3 min-w-0 text-foreground">
                        <span className="text-[10px] font-black text-muted-foreground uppercase shrink-0">{block.type === 'name' ? '节点名' : block.type === 'country' ? '国家' : block.type === 'protocol' ? '协议' : block.type === 'latency' ? '延迟' : block.type === 'status' ? '状态' : '订阅源'}</span>
                        <div className="flex-1 flex items-center gap-2">
                           {block.type === 'name' && (
                             <>
                               <button
                                 onClick={() => setBlocks(blocks.map(b => {
                                   if (b.id !== block.id) return b;
                                   const operator = nextNameOperator(b.operator);
                                   return { ...b, operator, value: operator === 'in' ? '' : b.value, values: operator === 'in' ? [] : b.values };
                                 }))}
                                 className="px-2 py-1 bg-muted rounded-lg text-[9px] font-black uppercase shrink-0 border"
                               >
                                 {nameOperatorLabel(block.operator)}
                               </button>
                               {(block.operator === 'equals' || block.operator === 'in') ? (
                                 <StyledSelect
                                   values={block.values}
                                   placeholder="选择完整节点..."
                                   options={nodeOptions}
                                   onChange={values => setBlocks(blocks.map(b => b.id === block.id ? { ...b, operator: 'in', value: '', values: values as string[] } : b))}
                                   className="flex-1"
                                   multiple
                                 />
                               ) : (
                                 <input value={block.value} onChange={e => setBlocks(blocks.map(b => b.id === block.id ? { ...b, value: e.target.value } : b))} placeholder="输入关键字..." className="flex-1 bg-background border-2 border-muted rounded-xl px-3 py-1.5 text-[11px] font-bold focus:border-primary/40 outline-none" />
                               )}
                             </>
                           )}
                           {block.type === 'country' && (
                             <StyledSelect
                               values={block.values}
                               placeholder="选择地区..."
                               options={countryOptions}
                               onChange={values => setBlocks(blocks.map(b => b.id === block.id ? { ...b, operator: 'in', value: '', values: values as string[] } : b))}
                               className="w-full"
                               multiple
                             />
                           )}
                           {block.type === 'latency' && (
                             <div className="relative w-full flex items-center gap-2"><span className="text-[10px] font-bold text-muted-foreground">低于</span><input type="number" value={block.value} onChange={e => setBlocks(blocks.map(b => b.id === block.id ? { ...b, value: e.target.value } : b))} className="flex-1 bg-background border-2 border-muted rounded-xl px-3 py-1.5 text-[11px] font-bold outline-none" /><span className="text-[10px] font-black text-muted-foreground uppercase tracking-wide">ms</span></div>
                           )}
                           {block.type === 'status' && (<div className="flex items-center gap-2 font-black text-[10px] text-muted-foreground uppercase"><ShieldAlert className="size-3" /> 匹配所有超时节点</div>)}
                           {block.type === 'protocol' && (
                             <StyledSelect
                               value={block.value}
                               placeholder="选择协议..."
                               options={protocolOptions}
                               onChange={value => setBlocks(blocks.map(b => b.id === block.id ? { ...b, value: value as string } : b))}
                               className="w-full"
                             />
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
             <div className="flex justify-between items-center text-muted-foreground uppercase tracking-wider font-black text-[10px]">
                <div className="flex items-center gap-3"><CheckCircle2 className="size-3" /> 03. 资产捕获实时预览</div>
                <span>{previewNodes.length} 命中</span>
             </div>
             <div className="bg-muted/10 rounded-[2rem] border-2 border-dashed p-4 min-h-[150px] max-h-[300px] overflow-y-auto space-y-1 custom-scrollbar text-foreground">
                {previewNodes.slice(0, 100).map(node => {
                  const dName = displayRuntimeName(node.name, node.displayName);
                  const sName = node.subscriptionName;
                  return (
                    <div key={node.name} className="flex items-center justify-between p-2.5 bg-background rounded-xl border border-border/60 text-foreground animate-in fade-in zoom-in-95">
                       <div className="flex items-center gap-2 min-w-0">
                         <p className="text-[11px] font-black truncate">{dName}</p>
                         {sName && <SubBadge name={sName} />}
                       </div>
                       <div className="flex items-center gap-2 shrink-0">
                          <div className={cn("size-1.5 rounded-full", node.latency > 0 ? "bg-green-500" : "bg-rose-500")} />
                          <span className="text-[9px] font-black text-muted-foreground uppercase">{node.latency > 0 ? `${node.latency}ms` : 'T.O'}</span>
                       </div>
                    </div>
                  );
                })}
             </div>
          </section>
        </div>
        <div className="p-6 md:p-8 bg-card border-t border-border/50 flex gap-4 shrink-0">
           <Button variant="ghost" onClick={onClose} className="h-14 px-8 rounded-2xl font-black text-xs uppercase tracking-widest flex-1 hover:bg-muted text-foreground">取消操作</Button>
           <Button onClick={() => void handleSave()} disabled={!name.trim() || isSaving} className="h-14 px-12 rounded-2xl font-black text-xs tracking-wider shadow-2xl shadow-primary/30 flex-[2] bg-primary text-primary-foreground transition-all hover:scale-[1.02] active:scale-95">{isSaving ? <Loader2 className="size-4 animate-spin" /> : initialData ? '保存修改' : '确认新增'}</Button>
        </div>
      </div>
    </div>
  );
};

export const Proxies = () => {
  const { toast } = useToast();
  const [groups, setGroups] = useState<ProxyGroup[]>([]);
  const [nodes, setNodes] = useState<ProxyNode[]>([]);
  const [activeGroupName, setActiveGroupName] = useState<string | null>(null);
  const [expandedGroupMobile, setExpandedGroupMobile] = useState<string | null>(null);
  const [isCreating, setIsCreating] = useState(false);
  const [editingData, setEditingData] = useState<ProxyGroup | null>(null);
  const [loading, setLoading] = useState(true);
  const [searchGroup, setSearchGroup] = useState('');
  const [searchNode, setSearchNode] = useState('');
  const [isSwitching, setIsSwitching] = useState<string | null>(null);
  const [testingGroup, setTestingGroup] = useState<string | null>(null);
  const [testingNode, setTestingNode] = useState<string | null>(null);
  const [presetStrategy, setPresetStrategy] = useState('url-test');
  const [updatingPresets, setUpdatingPresets] = useState(false);
  const presetBootstrapAttempted = useRef(false);

  const fetchData = useCallback(async () => {
    try {
      const data = await api.proxyTopology();
      setGroups(data.groups); setNodes(data.nodes);
      setActiveGroupName(current => data.groups.some(group => group.name === current) ? current : (data.groups[0]?.name ?? ''));
    } catch {
      toast('代理拓扑加载失败', 'error');
    } finally { setLoading(false); }
  }, [toast]);

  useEffect(() => {
    queueMicrotask(() => void fetchData());
  }, [fetchData]);

  const activeGroup = useMemo(() => groups.find(g => g.name === activeGroupName), [groups, activeGroupName]);
  const displayNameByRuntimeName = useMemo(() => new Map<string, string>([
    ...groups.map(group => [group.name, displayRuntimeName(group.name, group.displayName)] as const),
    ...nodes.map(node => [node.name, displayRuntimeName(node.name, node.displayName)] as const),
  ]), [groups, nodes]);
  const subscriptionByAsset = useMemo(() => new Map<string, string>([
    ...groups.flatMap(group => group.subscriptionName ? [[group.name, group.subscriptionName] as const] : []),
    ...nodes.flatMap(node => node.subscriptionName ? [[node.name, node.subscriptionName] as const] : []),
  ]), [groups, nodes]);

  const getNodesForGroup = useCallback((groupName: string) => {
    const group = groups.find(g => g.name === groupName);
    if (!group) return [];
    const query = searchNode.toLowerCase();
    return nodes.filter(n => group.all.includes(n.name)).filter(n => [
      displayRuntimeName(n.name, n.displayName),
      n.subscriptionName ?? '',
    ].some(value => value.toLowerCase().includes(query))).sort((a, b) => (group.type === 'select' ? 0 : (a.latency || 9999) - (b.latency || 9999)));
  }, [groups, nodes, searchNode]);

  const getGroupsForGroup = useCallback((groupName: string) => {
    const group = groups.find(item => item.name === groupName);
    if (!group) return [];
    const query = searchNode.toLowerCase();
    return groups.filter(item => item.name !== groupName && group.all.includes(item.name) && [
      displayRuntimeName(item.name, item.displayName),
      item.subscriptionName ?? '',
    ].some(value => value.toLowerCase().includes(query)));
  }, [groups, searchNode]);

  const activeNodes = useMemo(() => {
    if (!activeGroup) return [];
    return getNodesForGroup(activeGroup.name);
  }, [activeGroup, getNodesForGroup]);
  const activeMemberGroups = useMemo(() => activeGroup ? getGroupsForGroup(activeGroup.name) : [], [activeGroup, getGroupsForGroup]);

  const handleSelectNode = async (groupName: string, nodeName: string) => {
    const group = groups.find(g => g.name === groupName);
    if (!group || group.type !== 'select') return;
    setIsSwitching(nodeName);
    try {
      await api.selectProxy(group.name, nodeName);
      toast('出口已切换', 'success'); fetchData();
    } catch { toast('切换失败', 'error'); } finally { setIsSwitching(null); }
  };

  const handleTestGroup = async (e: ReactMouseEvent<HTMLButtonElement>, groupName: string) => {
    if (e) e.stopPropagation();
    setTestingGroup(groupName);
    toast(`正在刷新延迟...`, 'info');
    try {
      const results = await api.testProxyGroup(groupName);
      const delays = new Map(results.map(item => [item.name, item.delay]));
      setNodes(prev => prev.map(node => delays.has(node.name) ? { ...node, latency: delays.get(node.name) ?? node.latency } : node));
      await fetchData();
      toast('分组测速完成', 'success');
    } catch {
      toast('分组测速失败', 'error');
    } finally {
      setTestingGroup(null);
    }
  };

  const handleTestNode = async (e: ReactMouseEvent<HTMLButtonElement>, nodeName: string) => {
    if (e) e.stopPropagation();
    setTestingNode(nodeName);
    try {
      const result = await api.testNode(nodeName);
      setNodes(prev => prev.map(n => n.name === nodeName ? { ...n, latency: result.delay } : n));
    } catch {
      toast('节点测速失败', 'error');
    } finally {
      setTestingNode(null);
    }
  };

  const handleSaveGroup = async (data: ProxyGroupInput) => {
    if (editingData && isManagedGroup(editingData)) {
      toast('系统内置或托管分组不可编辑', 'error');
      return false;
    }
    try {
      if (editingData) {
        await api.updateProxyGroup(editingData.name, data);
      } else {
        await api.createProxyGroup(data);
      }
      await fetchData();
      setIsCreating(false);
      setEditingData(null);
      toast('分组配置已同步', 'success');
      return true;
    } catch {
      toast('分组配置同步失败', 'error');
      return false;
    }
  };

  const countryPresets = useMemo(() => groups.filter(group => presetCountry(group)), [groups]);
  const availableCountries = useMemo(() => Array.from(new Set(nodes.map(node => node.country).filter((country): country is string => Boolean(country)))).sort(), [nodes]);

  const handleCreateCountryPresets = async () => {
    const existing = new Set(countryPresets.map(group => presetCountry(group)));
    const missing = availableCountries.filter(country => !existing.has(country));
    if (missing.length === 0) return toast('当前国家预设已齐全', 'info');
    setUpdatingPresets(true);
    try {
      localStorage.removeItem(COUNTRY_PRESET_DISABLED_KEY);
      for (const country of missing) {
        await api.createProxyGroup({
          name: `${COUNTRY_PRESET_PREFIX}${country}`,
          type: 'url-test',
          filter: [{ action: 'keep', type: 'country', operator: 'in', values: [country], enabled: true }],
        });
      }
      await fetchData();
      toast(`已创建 ${missing.length} 个自动测速国家分组`, 'success');
    } catch {
      await fetchData();
      toast('部分国家分组创建失败，请检查重名分组', 'error');
    } finally { setUpdatingPresets(false); }
  };

  const handleUpdateCountryPresets = async () => {
    if (countryPresets.length === 0) return toast('暂无预设国家分组', 'info');
    setUpdatingPresets(true);
    try {
      await Promise.all(countryPresets.map(group => api.updateProxyGroup(group.name, {
        name: group.name, type: presetStrategy, filter: group.filter,
      })));
      await fetchData();
      toast(`已将 ${countryPresets.length} 个国家分组改为 ${presetStrategy}`, 'success');
    } catch { toast('预设策略修改失败', 'error'); }
    finally { setUpdatingPresets(false); }
  };

  const handleDeleteCountryPresets = async () => {
    if (countryPresets.length === 0) return toast('暂无预设国家分组', 'info');
    if (!window.confirm(`确定删除 ${countryPresets.length} 个预设国家分组？`)) return;
    setUpdatingPresets(true);
    try {
      await Promise.all(countryPresets.map(group => api.deleteProxyGroup(group.name)));
      localStorage.setItem(COUNTRY_PRESET_DISABLED_KEY, 'true');
      await fetchData();
      toast('预设国家分组已删除', 'success');
    } catch { toast('部分预设仍被路由引用，无法删除', 'error'); }
    finally { setUpdatingPresets(false); }
  };

  useEffect(() => {
    if (loading || updatingPresets || presetBootstrapAttempted.current || availableCountries.length === 0 || countryPresets.length > 0) return;
    presetBootstrapAttempted.current = true;
    if (localStorage.getItem(COUNTRY_PRESET_DISABLED_KEY) !== 'true') queueMicrotask(() => void handleCreateCountryPresets());
    // The bootstrap intentionally runs only once for the first loaded topology.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loading, updatingPresets, availableCountries.length, countryPresets.length]);

  const filteredGroups = useMemo(() => {
    const query = searchGroup.toLowerCase();
    return groups.filter(group => [
      displayRuntimeName(group.name, group.displayName),
      group.subscriptionName ?? '',
    ].some(value => value.toLowerCase().includes(query)));
  }, [groups, searchGroup]);

  if (loading) return <div className="flex items-center justify-center h-[60vh]"><Loader2 className="animate-spin text-primary size-10" /></div>;

  return (
    <div className="max-w-[1600px] mx-auto h-[calc(100vh-6rem)] md:h-[calc(100vh-8rem)] flex flex-col gap-4 animate-in fade-in duration-500 px-3 md:px-4 pb-6 overflow-hidden text-left">
      <div className="flex flex-col md:flex-row justify-between items-start md:items-end shrink-0 px-1 gap-4 text-foreground text-left">
        <div className="text-left">
          <div className="flex items-center gap-2"><h2 className="text-2xl md:text-3xl font-black tracking-tight">分组管理</h2><div className="size-2 rounded-full bg-green-500 animate-pulse shadow-[0_0_8px_rgba(34,197,94,0.6)]" /><div className="hidden sm:flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-amber-500/5 border border-amber-500/10 text-amber-600/80 text-[10px] font-black uppercase ml-2"><Lock className="size-3" /> 锁标代表系统内置或订阅托管，不可编辑</div></div>
          <p className="text-xs text-muted-foreground mt-1 flex items-center gap-1.5 font-medium"><Radio className="size-3.5" /> 物理出口调度中心</p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-[10px] font-bold text-muted-foreground">国家分组批量操作</span>
          <Button title="按当前节点国家补齐缺少的自动测速分组" onClick={handleCreateCountryPresets} disabled={updatingPresets || availableCountries.length === 0} variant="outline" className="h-10 rounded-xl gap-2 text-xs font-bold"><Flag className="size-4" /> 补齐国家分组</Button>
          <select title="选择要批量应用到国家分组的策略" aria-label="国家分组批量策略" value={presetStrategy} onChange={event => setPresetStrategy(event.target.value)} className="h-10 rounded-xl border bg-background px-3 text-xs font-bold outline-none focus:border-primary">
            {GROUP_STRATEGIES.map(strategy => <option key={strategy} value={strategy}>{GROUP_STRATEGY_LABELS[strategy]}</option>)}
          </select>
          <Button title={`应用到 ${countryPresets.length} 个国家分组`} onClick={handleUpdateCountryPresets} disabled={updatingPresets || countryPresets.length === 0} variant="outline" className="h-10 rounded-xl gap-2 text-xs font-bold"><RotateCcw className="size-4" /> 应用策略 ({countryPresets.length})</Button>
          <Button onClick={handleDeleteCountryPresets} disabled={updatingPresets || countryPresets.length === 0} variant="outline" title="删除全部自动创建的国家分组" className="h-10 rounded-xl gap-2 px-3 text-xs font-bold text-red-500"><Trash2 className="size-4" /> 删除国家分组</Button>
          <Button onClick={() => { setEditingData(null); setIsCreating(true); }} className="rounded-xl shadow-lg shadow-primary/20 h-10 px-4 text-xs bg-primary text-primary-foreground font-bold transition-all active:scale-95"><Plus className="size-4 md:mr-1.5" /> 新建分组</Button>
        </div>
      </div>

      <div className="hidden lg:flex flex-1 gap-4 overflow-hidden mt-2 min-h-0 text-foreground text-left">
        {/* Group List */}
        <div className="w-[380px] xl:w-[420px] flex flex-col bg-card border border-border rounded-[2rem] overflow-hidden shadow-md shrink-0">
          <div className="p-5 border-b border-border/50 bg-muted/20 shrink-0 text-left"><div className="relative"><Search className="absolute left-3.5 top-1/2 -translate-y-1/2 size-4 text-muted-foreground opacity-70" /><input value={searchGroup} onChange={(e) => setSearchGroup(e.target.value)} placeholder="搜索分组..." className="w-full pl-10 pr-4 py-2.5 bg-background border border-border/50 rounded-xl text-sm font-medium outline-none focus:border-primary/50 transition-all text-foreground" /></div></div>
          <div className="flex-1 overflow-y-auto p-3 space-y-2 custom-scrollbar">
            {filteredGroups.map(group => {
              const isActive = activeGroupName === group.name;
              const isSystemBuiltin = isSystemBuiltinGroup(group);
              const isReadOnly = isManagedGroup(group);
              const dName = displayRuntimeName(group.name, group.displayName);
              const sName = group.subscriptionName;
              return (
                <div key={group.name} onClick={() => setActiveGroupName(group.name)} className={cn("relative flex flex-col p-4 rounded-[1.25rem] cursor-pointer transition-all border-2 text-left", isActive ? "bg-primary/10 border-primary/30 shadow-sm" : "bg-transparent border-transparent hover:bg-muted/50")}>
                  {isActive && <div className="absolute left-0 top-1/2 -translate-y-1/2 w-1.5 h-10 bg-primary rounded-r-full shadow-lg" />}
                  <div className="flex items-start justify-between gap-3 text-foreground">
                    <div className="flex items-center gap-3 min-w-0">
                      <div className={cn("size-10 rounded-xl flex items-center justify-center shrink-0", isActive ? "bg-primary text-primary-foreground" : "bg-card border text-muted-foreground")}><GroupIcon type={group.type} className="size-5" /></div>
                      <div className="min-w-0 text-left">
                        <div className="flex items-center gap-1.5">
                          <h4 title={dName} className={cn("line-clamp-2 break-all text-base font-bold leading-5 text-foreground", !isActive && "text-muted-foreground group-hover:text-foreground")}>{dName}</h4>
                          {sName && <SubBadge name={sName} />}
                          {isSystemBuiltin && <span className="px-1.5 py-0.5 rounded-md bg-amber-500/10 border border-amber-500/20 text-[9px] font-black text-amber-600 shrink-0">系统内置</span>}
                          {isReadOnly && <Lock className="size-3 text-amber-500/70" />}
                        </div>
                        <div className="text-[10px] font-semibold text-muted-foreground uppercase text-left">{GROUP_STRATEGY_LABELS[group.type] ?? group.type} · {group.all.length} 成员</div>
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
        <div className="flex-1 flex flex-col bg-card border border-border rounded-[2rem] overflow-hidden shadow-md min-h-0 text-left">
          {activeGroup ? (
            <>
              <div className="p-8 border-b border-border/50 bg-muted/5 shrink-0">
                <div className="flex flex-col 2xl:flex-row 2xl:items-center justify-between gap-6 text-foreground">
                  <div className="flex items-start gap-5 text-left">
                    <div className={cn("size-16 rounded-2xl flex items-center justify-center shadow-lg border-2 shrink-0", isManagedGroup(activeGroup) ? "bg-amber-500/10 border-amber-500/20 text-amber-600" : "bg-primary/10 border-primary/20 text-primary")}>
                       <GroupIcon type={activeGroup.type} className="size-8" />
                    </div>
                    <div className="min-w-0 text-left text-foreground">
                      <div className="flex items-center gap-3">
                          <h3 title={displayRuntimeName(activeGroup.name, activeGroup.displayName)} className="max-w-2xl break-all text-xl font-black md:text-2xl">{displayRuntimeName(activeGroup.name, activeGroup.displayName)}</h3>
                          {activeGroup.subscriptionName && <SubBadge name={activeGroup.subscriptionName} />}
                         {isSystemBuiltinGroup(activeGroup) && <span className="px-2 py-1 rounded-lg bg-amber-500/10 border border-amber-500/20 text-[10px] font-black text-amber-600 shrink-0">系统内置</span>}
                      </div>
                      <div className="flex items-center gap-2 mt-1.5 text-foreground">
                        <div className={cn("size-2 rounded-full", activeGroup.now ? "bg-green-500 shadow-[0_0_8px_rgba(34,197,94,0.5)]" : "bg-muted")} />
                        <span className="text-xs md:text-sm font-semibold text-muted-foreground">当前出口:</span>
                        <div className="flex items-center gap-1.5 min-w-0">
                           <span className="text-xs md:text-sm font-bold truncate">{displayNameByRuntimeName.get(activeGroup.now || 'DIRECT') ?? displayRuntimeName(activeGroup.now || 'DIRECT')}</span>
                           {activeGroup.now && subscriptionByAsset.get(activeGroup.now) && <SubBadge name={subscriptionByAsset.get(activeGroup.now)!} />}
                        </div>
                      </div>
                    </div>
                  </div>
                  <div className="flex items-center gap-2 md:gap-3 w-full 2xl:w-auto shrink-0">
                    <div className="relative flex-1 2xl:w-64"><Search className="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground opacity-70" /><input value={searchNode} onChange={(e) => setSearchNode(e.target.value)} placeholder="过滤节点..." className="w-full h-10 pl-9 pr-3 py-2 bg-background border border-border/50 rounded-xl text-sm font-medium outline-none focus:border-primary/50 transition-all shadow-inner text-foreground" /></div>
                    {!isManagedGroup(activeGroup) && <Button onClick={() => { setEditingData(activeGroup); setIsCreating(true); }} variant="outline" size="icon" className="size-10 rounded-xl bg-background border-border/50 hover:bg-muted shrink-0 text-foreground"><Edit3 className="size-4" /></Button>}
                    <Button onClick={(e) => handleTestGroup(e, activeGroup.name)} disabled={testingGroup === activeGroup.name} variant="outline" className="h-10 rounded-xl gap-2 font-bold text-xs bg-background border-border/50 hover:bg-muted transition-all shrink-0 text-foreground">{testingGroup === activeGroup.name ? <Loader2 className="size-4 animate-spin" /> : <RotateCcw className="size-4 text-primary" />} 测速</Button>
                  </div>
                </div>
              </div>
              <div className="flex-1 overflow-y-auto p-6 custom-scrollbar text-left text-foreground">
                {activeGroup.type === 'select' ? (
                  <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 gap-4 text-left">
                    {activeMemberGroups.map(member => {
                      const isActive = activeGroup.now === member.name;
                      return (
                        <div key={member.name} className={cn("relative flex min-h-28 flex-col p-4 rounded-2xl border-2 transition-all duration-200 text-left overflow-hidden", isActive ? "bg-primary/10 border-primary shadow-sm text-foreground font-bold" : "bg-background border-border/50 hover:border-primary/40 text-muted-foreground hover:text-foreground")}>
                          <button type="button" aria-label={`选择策略组 ${displayRuntimeName(member.name, member.displayName)}`} aria-pressed={isActive} onClick={() => handleSelectNode(activeGroup.name, member.name)} disabled={!!isSwitching} className="absolute inset-0 z-0 rounded-2xl focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" />
                          <div className="relative z-10 pointer-events-none flex h-full flex-col justify-between gap-3">
                            <div className="flex items-center justify-between"><span className="inline-flex items-center gap-1 rounded-md bg-primary/10 px-2 py-1 text-[9px] font-black text-primary"><Layers className="size-3" /> 策略组</span>{isActive && <CheckCircle2 className="size-4 text-primary" />}</div>
                            <div className="min-w-0"><p title={displayRuntimeName(member.name, member.displayName)} className="line-clamp-2 min-h-10 break-all text-sm font-bold">{displayRuntimeName(member.name, member.displayName)}</p><p className="mt-1 text-[9px] font-bold text-muted-foreground">{GROUP_STRATEGY_LABELS[member.type] ?? member.type} · {member.all.length} 成员</p></div>
                          </div>
                          {isSwitching === member.name && <div className="absolute inset-0 z-20 bg-background/50 flex items-center justify-center"><Loader2 className="size-5 animate-spin text-primary" /></div>}
                        </div>
                      );
                    })}
                    {activeNodes.map(node => {
                      const dName = displayRuntimeName(node.name, node.displayName);
                      const sName = node.subscriptionName;
                      return (
                        <div key={node.name} className={cn("relative flex flex-col p-4 rounded-2xl border-2 transition-all duration-200 text-left overflow-hidden group/node", activeGroup.now === node.name ? "bg-primary/10 border-primary shadow-sm text-foreground font-bold" : "bg-background border-border/50 hover:border-primary/40 text-muted-foreground hover:text-foreground")}>
                          <button
                            type="button"
                            aria-label={`选择节点 ${dName}`}
                            aria-pressed={activeGroup.now === node.name}
                            onClick={() => handleSelectNode(activeGroup.name, node.name)}
                            disabled={!!isSwitching}
                            className="absolute inset-0 z-0 rounded-2xl focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                          />
                          <div className="relative z-10 pointer-events-none">
                            <div className="flex justify-between items-center w-full mb-3 text-foreground"><StatusBadge delay={node.latency} loading={testingNode === node.name} onClick={(e) => handleTestNode(e, node.name)} className={cn("pointer-events-auto", activeGroup.now === node.name ? "bg-background border-primary/20 text-primary" : "")} />{activeGroup.now === node.name && <CheckCircle2 className="size-4 text-primary" />}</div>
                            <div className="min-w-0 w-full"><p title={dName} className="line-clamp-2 min-h-10 break-all text-sm font-bold">{dName}</p>{sName && <div className="mt-1"><SubBadge name={sName} /></div>}</div>
                          </div>
                          {isSwitching === node.name && <div className="absolute inset-0 z-20 bg-background/50 backdrop-blur-sm flex items-center justify-center"><Loader2 className="size-5 animate-spin text-primary" /></div>}
                        </div>
                      );
                    })}
                  </div>
                ) : (
                  <div className="space-y-2">
                    <div className="flex items-center gap-4 px-4 py-2 text-[10px] font-black uppercase text-muted-foreground border-b mb-4"><div className="w-12 text-center">Rank</div><div className="flex-1 text-left">Identity</div><div className="w-24 text-right">Latency</div></div>
                    {activeMemberGroups.map((member, idx) => (
                      <div key={member.name} className={cn("flex items-center justify-between p-3.5 rounded-2xl border transition-all text-foreground", activeGroup.now === member.name ? "bg-primary/10 border-primary shadow-sm font-bold" : "bg-background border-border/40 text-muted-foreground")}>
                        <div className="flex items-center gap-5 flex-1 min-w-0"><div className="w-8 h-8 rounded-lg bg-primary/10 text-primary flex items-center justify-center"><Layers className="size-4" /></div><div className="min-w-0"><p title={displayRuntimeName(member.name, member.displayName)} className="line-clamp-2 break-all text-sm font-bold">{displayRuntimeName(member.name, member.displayName)}</p><p className="text-[10px] text-muted-foreground">策略组 · {GROUP_STRATEGY_LABELS[member.type] ?? member.type}</p></div></div>
                        <span className="text-[10px] font-bold text-muted-foreground">#{idx + 1}</span>
                      </div>
                    ))}
                    {activeNodes.map((node, idx) => {
                      const dName = displayRuntimeName(node.name, node.displayName);
                      const sName = node.subscriptionName;
                      return (
                        <div key={node.name} className={cn("flex items-center justify-between p-3.5 rounded-2xl border transition-all text-foreground", activeGroup.now === node.name ? "bg-primary/10 border-primary shadow-sm font-bold" : "bg-background border-border/40 hover:bg-card hover:border-border text-muted-foreground")}>
                          <div className="flex items-center gap-5 flex-1 min-w-0 text-left text-foreground"><div className={cn("w-8 h-8 rounded-lg flex items-center justify-center font-bold text-xs shrink-0 text-foreground", activeGroup.now === node.name ? "bg-primary text-white shadow-sm" : "bg-muted text-muted-foreground")}>{idx + 1}</div><div className="min-w-0 flex-1 text-foreground text-left"><div className="flex items-start gap-2"><p title={dName} className="line-clamp-2 break-all text-sm font-bold">{dName}</p>{sName && <SubBadge name={sName} />}</div><p className="text-[10px] text-muted-foreground uppercase mt-0.5">{node.type}</p></div></div>
                          <StatusBadge delay={node.latency} loading={testingNode === node.name} onClick={(e) => handleTestNode(e, node.name)} className={activeGroup.now === node.name ? "bg-background border-primary/20 text-primary" : ""} />
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            </>
          ) : (
            <div className="flex-1 flex flex-col items-center justify-center text-muted-foreground opacity-80"><Globe className="size-20 mb-6" /><p className="text-xl font-bold tracking-widest uppercase text-center">Select a Group</p></div>
          )}
        </div>
      </div>

      {/* Mobile View */}
      <div className="lg:hidden flex-1 min-h-0 flex flex-col gap-3 pb-24 overflow-y-auto mt-2 text-left text-foreground">
        <div className="sticky top-0 z-20 bg-background py-2 shrink-0"><div className="relative"><Search className="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground opacity-70" /><input value={searchGroup} onChange={(e) => setSearchGroup(e.target.value)} placeholder="搜索分组..." className="w-full pl-9 pr-3 py-3 bg-card border border-border/60 rounded-xl text-sm font-medium outline-none focus:border-primary/50 shadow-sm text-foreground" /></div></div>
        {filteredGroups.map(group => {
          const isExpanded = expandedGroupMobile === group.name;
          const isSystemBuiltin = isSystemBuiltinGroup(group);
          const isReadOnly = isManagedGroup(group);
          const dName = displayRuntimeName(group.name, group.displayName);
          const sName = group.subscriptionName;
          return (
            <div key={group.name} className={cn("flex flex-col shrink-0 rounded-[1.25rem] border-2 transition-all overflow-hidden text-foreground", isExpanded ? "bg-card border-primary/30 shadow-md" : "bg-card border-border/50")}>
              <div onClick={() => setExpandedGroupMobile(isExpanded ? null : group.name)} className="flex items-center justify-between p-4 cursor-pointer select-none text-foreground text-left"><div className="flex items-center gap-3 min-w-0 text-foreground text-left"><div className={cn("size-10 rounded-xl flex items-center justify-center shrink-0 shadow-sm text-foreground", isExpanded ? "bg-primary/10 text-primary" : "bg-muted/50 border text-muted-foreground")}><GroupIcon type={group.type} className="size-5 text-foreground" /></div><div className="min-w-0 text-left text-foreground"><div className="flex items-start gap-1.5 text-left text-foreground"><h4 title={dName} className="line-clamp-2 break-all text-base font-bold leading-5 text-foreground">{dName}</h4>{sName && <SubBadge name={sName} />}{isSystemBuiltin && <span className="px-1.5 py-0.5 rounded-md bg-amber-500/10 border border-amber-500/20 text-[9px] font-black text-amber-600 shrink-0">系统内置</span>}{isReadOnly && <Lock className="size-3 text-amber-500/70 shrink-0" />}</div><div className="text-[10px] font-semibold text-muted-foreground uppercase text-left">{GROUP_STRATEGY_LABELS[group.type] ?? group.type} · {group.all.length} 成员</div></div></div><div className="flex items-center gap-3 shrink-0 text-foreground"><StatusBadge delay={group.delay} loading={testingGroup === group.name} onClick={(e) => handleTestGroup(e, group.name)} /><ChevronDown className={cn("size-4 text-muted-foreground transition-transform", isExpanded && "rotate-180 text-primary")} /></div></div>
              {isExpanded && (
                <div className="border-t bg-background/30 p-3 animate-in slide-in-from-top-2 duration-300 text-foreground text-left">
                  <div className="flex gap-2 mb-3 text-foreground text-left"><div className="relative flex-1"><Search className="absolute left-2.5 top-1/2 -translate-y-1/2 size-3.5 text-muted-foreground opacity-70" /><input value={searchNode} onChange={(e) => setSearchNode(e.target.value)} placeholder="过滤节点..." className="w-full pl-8 pr-3 py-2 bg-background border border-border/50 rounded-lg text-xs outline-none focus:border-primary/50 text-foreground" /></div>{!isReadOnly && <Button onClick={() => { setEditingData(group); setIsCreating(true); }} variant="outline" size="icon" className="h-9 w-9 text-foreground"><Edit3 className="size-3.5 text-foreground" /></Button>}<Button onClick={(e) => handleTestGroup(e, group.name)} disabled={testingGroup === group.name} variant="outline" className="h-9 px-3 rounded-lg gap-1.5 font-bold text-xs text-foreground">测速</Button></div>
                  <div className="space-y-2 max-h-[50vh] overflow-y-auto pr-1 text-left custom-scrollbar text-foreground">
                    {getGroupsForGroup(group.name).map(member => {
                      const isActive = group.now === member.name;
                      return (
                        <button key={member.name} type="button" onClick={() => handleSelectNode(group.name, member.name)} disabled={!!isSwitching || group.type !== 'select'} className={cn("w-full flex items-center justify-between p-3 rounded-xl border text-left", isActive ? "bg-primary/10 border-primary" : "bg-card border-border/50")}>
                          <span className="flex min-w-0 items-center gap-2"><Layers className="size-4 shrink-0 text-primary" /><span className="break-all text-xs font-bold">{displayRuntimeName(member.name, member.displayName)}</span></span><span className="text-[9px] font-black text-primary">策略组</span>
                        </button>
                      );
                    })}
                    {getNodesForGroup(group.name).map(node => {
                       const isNodeActive = group.now === node.name;
                       const dN = displayRuntimeName(node.name, node.displayName);
                       const sN = node.subscriptionName;
                       return (
                          <div key={node.name} className={cn("relative w-full flex items-center justify-between p-3 rounded-xl border transition-all text-left", isNodeActive ? "bg-primary/10 border-primary shadow-sm text-foreground font-bold" : "bg-card border-border/50 text-muted-foreground")}>
                            <button
                              type="button"
                              aria-label={`选择节点 ${dN}`}
                              aria-pressed={isNodeActive}
                              onClick={() => handleSelectNode(group.name, node.name)}
                              disabled={!!isSwitching}
                              className="absolute inset-0 z-0 rounded-xl focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                            />
                            <div className="relative z-10 pointer-events-none min-w-0 flex-1 text-left"><div className="flex items-start gap-2"><p className="break-all text-xs">{dN}</p>{sN && <SubBadge name={sN} />}</div></div>
                            <StatusBadge delay={node.latency} loading={testingNode === node.name} onClick={(e) => handleTestNode(e, node.name)} className={cn("relative z-10 pointer-events-auto", isNodeActive ? "bg-background border-primary/20 text-primary" : "")} />
                          </div>
                       )
                    })}
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>
      {isCreating && (
        <CreateGroupDrawer
          key={editingData?.name ?? 'new-group'}
          isOpen
          onClose={() => { setIsCreating(false); setEditingData(null); }}
          onSave={handleSaveGroup}
          allNodes={nodes}
          initialData={editingData}
        />
      )}
    </div>
  );
};
