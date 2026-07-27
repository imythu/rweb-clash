import { useState, useEffect, useMemo, useRef, useCallback } from 'react';
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
  Filter,
  Eye,
  Server,
  Network,
  Search
} from 'lucide-react';
import { Button } from "@/components/ui/button";
import { useToast } from './toast-context';
import { cn, SUB_DELIMITER } from '@/lib/utils';
import { api, type DownloadRoute, type FilterRule, type FilterRuleInput, type Subscription, type SubscriptionInput, type SubscriptionMembers } from '@/lib/api';
import { Spinner } from '@/components/ui/spinner';
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';

const displayAssetName = (name: string) => name.split(SUB_DELIMITER)[0] || name;
const displaySubscriptionFormat = (format?: string | null) => (format?.trim() || 'auto').toUpperCase();

type SelectOption = {
  value: string;
  label: string;
  meta?: string;
};

type MemberSection = SubscriptionMembers['filtered'];
type MemberViewFilter = 'all' | 'imported' | 'rejected';
type EditableFilterRule = Omit<FilterRule, 'id'> & { id?: string; values: string[] };
type SubscriptionDraft = Pick<Subscription, 'name' | 'url' | 'traffic' | 'intervalSeconds' | 'inheritGlobal' | 'downloadRoute'> &
  Partial<Pick<Subscription, 'id' | 'format' | 'breakdown'>> & {
    rules: EditableFilterRule[];
  };

const EMPTY_MEMBER_SECTION: MemberSection = { nodes: [], groups: [] };
const EMPTY_SUBSCRIPTION_DRAFT: SubscriptionDraft = {
  name: '',
  url: '',
  rules: [],
  traffic: { used: 0, total: 100 * 1024 ** 3 },
  intervalSeconds: 21_600,
  inheritGlobal: true,
  downloadRoute: 'auto',
};

const DOWNLOAD_ROUTE_LABELS: Record<DownloadRoute, string> = {
  auto: '自动回退',
  direct: '直连',
  core: '当前内核',
  system: '系统代理',
};

const formatLatency = (latency: number) => latency > 0 ? `${latency}ms` : 'T.O';
const sectionCounts = (section: MemberSection) => section.nodes.length + section.groups.length;

const matchesMemberSearch = (values: Array<string | number | null | undefined>, query: string) =>
  values.some(value => String(value ?? '').toLowerCase().includes(query));

const filterMemberSection = (section: MemberSection, rawQuery: string): MemberSection => {
  const query = rawQuery.trim().toLowerCase();
  if (!query) return section;

  return {
    nodes: section.nodes.filter(node => matchesMemberSearch([
      node.name,
      node.displayName,
      displayAssetName(node.name),
      node.country,
      node.protocol,
      node.filterReason,
      node.latency > 0 ? `${node.latency}ms` : 'T.O',
    ], query)),
    groups: section.groups.filter(group => matchesMemberSearch([
      group.name,
      group.displayName,
      displayAssetName(group.name),
      group.type,
      group.memberCount,
      group.filterReason,
      ...group.members,
    ], query)),
  };
};

const AssetSection = ({ title, subtitle, section, rejected = false }: { title: string; subtitle: string; section: MemberSection; rejected?: boolean }) => (
  <section className="space-y-4">
    <div className="flex items-center justify-between gap-3">
      <div className="min-w-0">
        <h4 className="text-sm font-black uppercase tracking-tight">{title}</h4>
        <p className="text-[9px] font-bold text-muted-foreground mt-0.5">{subtitle}</p>
      </div>
      <div className="px-2.5 py-1 rounded-lg bg-primary/10 text-primary text-[9px] font-black shrink-0">{sectionCounts(section)} 项</div>
    </div>

    <div className="space-y-2">
      <div className="flex items-center gap-2 text-[10px] font-black text-muted-foreground uppercase tracking-widest">
        <Server className="size-3" /> 节点 · {section.nodes.length}
      </div>
      <div className="space-y-1.5">
        {section.nodes.map(node => (
          <div key={`${rejected ? 'rejected' : 'imported'}-${node.name}`} className={cn("rounded-xl border p-3 bg-background/70 flex items-center justify-between gap-3", node.filteredOut && "border-rose-500/20 bg-rose-500/[0.03]")}>
            <div className="min-w-0">
              <div className="flex items-center gap-2 min-w-0">
                <p className="text-xs font-black truncate">{node.displayName || displayAssetName(node.name)}</p>
                {node.filteredOut && <span className="px-1.5 py-0.5 rounded-md bg-rose-500/10 text-rose-600 text-[10px] font-black shrink-0">已剔除</span>}
              </div>
              <p className="text-[9px] font-mono text-muted-foreground truncate mt-0.5">{node.name}</p>
              {node.filterReason && <p className="text-[9px] font-bold text-rose-600/80 truncate mt-1">{node.filterReason}</p>}
            </div>
            <div className="shrink-0 text-right">
              <p className="text-[9px] font-black text-muted-foreground uppercase">{node.country || 'UNK'} · {node.protocol}</p>
              <p className="text-[9px] font-black text-primary mt-1">{formatLatency(node.latency)}</p>
            </div>
          </div>
        ))}
        {section.nodes.length === 0 && <div className="rounded-xl border-2 border-dashed border-muted p-6 text-center text-[10px] font-black text-muted-foreground">{rejected ? '暂无已剔除节点' : '暂无节点'}</div>}
      </div>
    </div>

    <div className="space-y-2 pt-2">
      <div className="flex items-center gap-2 text-[10px] font-black text-muted-foreground uppercase tracking-widest">
        <Network className="size-3" /> 订阅组 · {section.groups.length}
      </div>
      <div className="space-y-1.5">
        {section.groups.map(group => (
          <div key={`${rejected ? 'rejected' : 'imported'}-${group.name}`} className="rounded-xl border p-3 bg-background/70 space-y-2">
            <div className="flex items-center justify-between gap-3">
              <div className="min-w-0">
                <p className="text-xs font-black truncate">{displayAssetName(group.displayName || group.name)}</p>
                <p className="text-[9px] font-mono text-muted-foreground truncate mt-0.5">{group.name}</p>
              </div>
              <span className="px-2 py-1 rounded-lg bg-muted text-[9px] font-black shrink-0">{group.type} · {group.memberCount}</span>
            </div>
            <div className="flex flex-wrap gap-1.5">
              {group.members.slice(0, 8).map(member => (
                <span key={member} className="max-w-full px-2 py-1 rounded-lg bg-muted/60 text-[9px] font-bold text-muted-foreground truncate">{displayAssetName(member)}</span>
              ))}
              {group.members.length > 8 && <span className="px-2 py-1 rounded-lg bg-primary/10 text-primary text-[9px] font-black">+{group.members.length - 8}</span>}
            </div>
          </div>
        ))}
        {section.groups.length === 0 && (
          <div className="rounded-xl border-2 border-dashed border-muted p-5 text-center text-[10px] font-bold text-muted-foreground">
            {rejected ? '未保存已剔除订阅组快照；订阅组会按已导入节点重建。' : '暂无订阅组'}
          </div>
        )}
      </div>
    </div>
  </section>
);

const RuleMultiSelect = ({
  values = [],
  placeholder,
  options,
  onChange,
}: {
  values?: string[];
  placeholder: string;
  options: SelectOption[];
  onChange: (values: string[]) => void;
}) => {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const selectedValues = values.map(value => value.trim()).filter(Boolean);
  const selectedSet = new Set(selectedValues);
  const selectedOptions = options.filter(option => selectedSet.has(option.value));
  const displayText = selectedOptions.length === 0
    ? placeholder
    : selectedOptions.length === 1
      ? selectedOptions[0].label
      : `${selectedOptions.length} 项已选`;

  useEffect(() => {
    if (!open) return;
    const handlePointerDown = (event: PointerEvent) => {
      if (!ref.current?.contains(event.target as Node)) setOpen(false);
    };
    window.addEventListener('pointerdown', handlePointerDown);
    return () => window.removeEventListener('pointerdown', handlePointerDown);
  }, [open]);

  const toggleOption = (value: string) => {
    onChange(selectedSet.has(value)
      ? selectedValues.filter(item => item !== value)
      : [...selectedValues, value]);
  };

  return (
    <div ref={ref} className="relative flex-1 min-w-[160px]">
      <button
        type="button"
        onClick={() => setOpen(!open)}
        className={cn(
          "w-full bg-background border-2 rounded-xl px-3 py-2 text-left text-[12px] font-black transition-all outline-none shadow-inner flex items-center justify-between gap-2",
          open ? "border-primary/40 ring-4 ring-primary/10" : "border-muted-foreground/10 hover:border-primary/30"
        )}
      >
        <span className={cn("truncate", selectedOptions.length === 0 && "text-muted-foreground/30")}>{displayText}</span>
        <ChevronDown className={cn("size-3.5 shrink-0 text-muted-foreground transition-transform", open && "rotate-180 text-primary")} />
      </button>
      {open && (
        <div className="absolute left-0 right-0 top-[calc(100%+0.35rem)] z-[130] max-h-64 overflow-y-auto rounded-2xl border-2 border-border/70 bg-card p-1.5 shadow-2xl shadow-black/10 custom-scrollbar animate-in fade-in zoom-in-95 duration-150">
          {selectedOptions.length > 0 && (
            <div className="flex items-center justify-between gap-2 border-b border-border/60 px-2 pb-1.5 mb-1">
              <span className="text-[9px] font-black text-muted-foreground uppercase">{selectedOptions.length} selected</span>
              <button type="button" onClick={() => onChange([])} className="px-2 py-1 rounded-lg text-[9px] font-black text-red-500 hover:bg-red-500/10">清空</button>
            </div>
          )}
          {options.length === 0 ? (
            <div className="px-3 py-2 text-[10px] font-black text-muted-foreground/60">暂无可选节点</div>
          ) : options.map(option => (
            <button
              type="button"
              key={option.value}
              onClick={() => toggleOption(option.value)}
              className={cn(
                "w-full rounded-xl px-3 py-2 text-left transition-all flex items-center justify-between gap-3",
                selectedSet.has(option.value) ? "bg-primary/10 text-primary" : "hover:bg-muted/60 text-foreground"
              )}
            >
              <span className="min-w-0">
                <span className="block truncate text-[11px] font-black">{option.label}</span>
                {option.meta && <span className="block truncate text-[9px] font-mono text-muted-foreground/70 mt-0.5">{option.meta}</span>}
              </span>
              {selectedSet.has(option.value) && <CheckCircle2 className="size-3.5 shrink-0" />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
};

interface SubscriptionCardProps {
  sub: Subscription;
  onEdit: (subscription: Subscription) => void;
  onDelete: (id: string) => void;
  onMembers: (subscription: Subscription) => void;
}

const SubscriptionCard = ({ sub, onEdit, onDelete, onMembers }: SubscriptionCardProps) => {
  const { toast } = useToast();
  const copyUrl = () => { navigator.clipboard.writeText(sub.url); toast('地址已复制', 'success'); };
  const formatBytes = (bytes: number) => {
    if (!bytes) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
  };
  const trafficPercent = sub.traffic.total > 0 ? Math.min(100, (sub.traffic.used / sub.traffic.total) * 100) : 0;

  // Status mapping for visual cues
  const statusConfig: Record<string, { color: string; text: string }> = {
    online: { color: 'bg-emerald-500', text: '在线' },
    offline: { color: 'bg-rose-500', text: '失效' },
    syncing: { color: 'bg-blue-500', text: '同步中' }
  };
  const status = sub.status || 'online';
  const config = statusConfig[status] || statusConfig.online;

  return (
    <div className={cn(
      "relative p-[2px] rounded-[2rem] overflow-hidden group transition-all duration-500 hover:shadow-2xl hover:shadow-primary/10",
      status === 'syncing' ? "animate-border-flow" : "bg-transparent"
    )}>
      <div className="bg-card rounded-[1.95rem] overflow-hidden h-full flex flex-col border border-muted shadow-sm group-hover:shadow-md transition-shadow">
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
                <div className="flex items-center gap-1.5 mt-0.5 text-muted-foreground hover:text-foreground transition-colors cursor-pointer group/url" onClick={copyUrl}>
                  <Link className="size-2.5 shrink-0" />
                  <p className="text-[10px] font-mono truncate max-w-[80px] md:max-w-[140px]">{sub.url}</p>
                </div>
                <div className="mt-1 flex items-center gap-1.5">
                  <span className="text-[9px] font-black text-muted-foreground uppercase">订阅格式</span>
                  <span className="px-1.5 py-0.5 rounded-md bg-primary/10 text-primary text-[9px] font-black tracking-wide">
                    {displaySubscriptionFormat(sub.format)}
                  </span>
                  <span className="text-[9px] font-black text-muted-foreground">
                    路线 {DOWNLOAD_ROUTE_LABELS[(sub.lastRoute || sub.downloadRoute) as DownloadRoute] || sub.lastRoute}
                  </span>
                </div>
              </div>
            </div>
            <button onClick={() => onDelete(sub.id)} className="size-7 md:size-8 rounded-xl bg-destructive/5 text-destructive flex items-center justify-center opacity-0 group-hover:opacity-100 hover:bg-rose-500 hover:text-white transition-all shrink-0"><Trash2 className="size-3.5 md:size-4" /></button>
          </div>
        </div>

        {/* Bento Grid Content */}
        <div className="flex-1 grid grid-cols-2 gap-1.5 md:gap-2 p-2 md:p-4 pt-0">
          {/* Traffic Block */}
          <div className="col-span-2 bg-muted/40 rounded-xl md:rounded-2xl p-3 md:p-4 border border-border/60 hover:border-primary/20 transition-all">
            <div className="flex flex-col sm:flex-row justify-between items-start sm:items-end gap-1 mb-2">
              <span className="text-[10px] font-black text-muted-foreground uppercase tracking-wide">已用流量</span>
              <span className="text-[10px] md:text-xs font-black">{formatBytes(sub.traffic.used)} <span className="text-muted-foreground">/ {formatBytes(sub.traffic.total)}</span></span>
            </div>
            <div className="h-1.5 md:h-2 w-full bg-muted rounded-full overflow-hidden shadow-inner border border-background">
              <div className={cn("h-full transition-all duration-1000 ease-out", trafficPercent > 90 ? "bg-rose-500" : "bg-primary")} style={{ width: `${trafficPercent}%` }} />
            </div>
          </div>

          {/* Expiry Block */}
          <div className="bg-muted/30 rounded-xl md:rounded-2xl p-2 md:p-3 flex flex-col justify-between hover:bg-muted/50 transition-all border border-border/50">
             <span className="text-[10px] font-black text-muted-foreground uppercase">服务到期</span>
             <p className="text-[9px] md:text-[10px] font-black mt-0.5">{sub.expiry}</p>
          </div>

          {/* Status Block */}
          <div className="bg-muted/30 rounded-xl md:rounded-2xl p-2 md:p-3 flex flex-col justify-between hover:bg-muted/50 transition-all border border-border/50">
             <span className="text-[10px] font-black text-muted-foreground uppercase">连接状态</span>
             <div className="flex items-center gap-1.5 mt-0.5">
                <div className={cn("size-1.5 md:size-2 rounded-full", config.color, status === 'syncing' && "animate-pulse")} />
                <span className={cn("text-[10px] font-black uppercase", status === 'offline' ? "text-rose-600" : "text-emerald-600")}>{config.text}</span>
             </div>
          </div>
        </div>

        {/* Bento Footer */}
        <div className="px-3 md:px-4 py-2.5 md:py-3 bg-muted/5 border-t border-dashed flex items-center justify-between gap-2">
           <div className="flex items-baseline gap-1 min-w-0">
              <p className="text-xs md:text-sm font-black tracking-tighter truncate">{sub.nodes}</p>
              <span className="text-[10px] font-bold text-primary uppercase hidden xs:inline">Nodes</span>
           </div>
           <div className="flex items-center gap-1.5 shrink-0">
             <Button variant="outline" size="sm" onClick={() => onMembers(sub)} className="h-7 md:h-8 rounded-lg md:rounded-xl text-[10px] font-black uppercase border-2 px-2.5 md:px-3 shadow-sm hover:bg-muted transition-all shrink-0 gap-1">
               <Eye className="size-3" /> 成员
             </Button>
             <Button variant="outline" size="sm" onClick={() => onEdit(sub)} className="h-7 md:h-8 rounded-lg md:rounded-xl text-[10px] font-black uppercase border-2 px-2.5 md:px-4 shadow-sm hover:bg-primary hover:text-white hover:border-primary transition-all shrink-0">配置</Button>
           </div>
        </div>
      </div>

      {/* Sync Failure Snapshot Overlay (Only for Offline) */}
      {status === 'offline' && sub.lastError && (
        <div className="absolute top-12 left-6 right-6 bg-rose-500 text-white p-2 rounded-lg text-[10px] font-bold shadow-xl animate-in zoom-in-95 pointer-events-none z-10 border border-white/20">
           ERROR: {sub.lastError}
        </div>
      )}
    </div>
  );
};

const MembersDrawer = ({ sub, data, loading, onClose }: { sub: Subscription; data: SubscriptionMembers | null; loading: boolean; onClose: () => void }) => {
  const [viewFilter, setViewFilter] = useState<MemberViewFilter>('all');
  const [searchQuery, setSearchQuery] = useState('');
  const [filterOpen, setFilterOpen] = useState(false);
  const filterRef = useRef<HTMLDivElement>(null);
  const rejectedRawSection = useMemo<MemberSection>(() => (
    data ? { nodes: data.beforeFilter.nodes.filter(node => node.filteredOut), groups: [] } : EMPTY_MEMBER_SECTION
  ), [data]);
  const importedSection = useMemo(() => filterMemberSection(data?.filtered ?? EMPTY_MEMBER_SECTION, searchQuery), [data, searchQuery]);
  const rejectedSection = useMemo(() => filterMemberSection(rejectedRawSection, searchQuery), [rejectedRawSection, searchQuery]);
  const memberFilterOptions: Array<{ value: MemberViewFilter; label: string; count: number }> = [
    { value: 'all', label: '全部', count: sectionCounts(data?.filtered ?? EMPTY_MEMBER_SECTION) + sectionCounts(rejectedRawSection) },
    { value: 'imported', label: '已导入', count: sectionCounts(data?.filtered ?? EMPTY_MEMBER_SECTION) },
    { value: 'rejected', label: '已剔除', count: sectionCounts(rejectedRawSection) },
  ];
  const activeFilterOption = memberFilterOptions.find(option => option.value === viewFilter) ?? memberFilterOptions[0];
  const showImported = viewFilter === 'all' || viewFilter === 'imported';
  const showRejected = viewFilter === 'all' || viewFilter === 'rejected';

  useEffect(() => {
    if (!filterOpen) return;
    const handlePointerDown = (event: PointerEvent) => {
      if (!filterRef.current?.contains(event.target as Node)) setFilterOpen(false);
    };
    window.addEventListener('pointerdown', handlePointerDown);
    return () => window.removeEventListener('pointerdown', handlePointerDown);
  }, [filterOpen]);

  return (
    <div className="fixed inset-0 z-[60] flex justify-end overflow-hidden">
      <div className="absolute inset-0 bg-background/60 backdrop-blur-md" onClick={onClose} />
      <div className="relative w-full sm:max-w-2xl bg-card border-l h-full shadow-2xl flex flex-col animate-in slide-in-from-right duration-500 text-left">
        <div className="p-5 border-b flex justify-between items-center bg-muted/20 shrink-0">
          <div className="min-w-0">
            <h3 className="text-lg font-black uppercase tracking-tight truncate">订阅成员</h3>
            <p className="text-[9px] font-black text-primary uppercase tracking-widest truncate mt-1">{sub.name}</p>
          </div>
          <Button variant="ghost" size="icon" onClick={onClose} className="rounded-xl size-10 hover:bg-muted shrink-0"><X className="size-5" /></Button>
        </div>
        <div className="flex-1 overflow-y-auto p-5 md:p-6 custom-scrollbar">
          {loading ? (
            <div className="h-full min-h-[50vh] flex items-center justify-center"><Loader2 className="size-8 animate-spin text-primary" /></div>
          ) : data ? (
            <div className="space-y-6">
              <div className="flex flex-col sm:flex-row gap-2">
                <div ref={filterRef} className="relative sm:w-44">
                  <button
                    type="button"
                    onClick={() => setFilterOpen(!filterOpen)}
                    className={cn(
                      "w-full h-11 rounded-xl border-2 bg-background px-3 text-left shadow-inner transition-all flex items-center justify-between gap-2",
                      filterOpen ? "border-primary/40 ring-4 ring-primary/10" : "border-muted-foreground/10 hover:border-primary/30"
                    )}
                  >
                    <span className="min-w-0 flex items-center gap-2">
                      <Filter className="size-3.5 shrink-0 text-primary" />
                      <span className="truncate text-[11px] font-black">{activeFilterOption.label}</span>
                    </span>
                    <span className="flex items-center gap-1.5 shrink-0">
                      <span className="text-[9px] font-black text-muted-foreground">{activeFilterOption.count}</span>
                      <ChevronDown className={cn("size-3.5 text-muted-foreground transition-transform", filterOpen && "rotate-180 text-primary")} />
                    </span>
                  </button>
                  {filterOpen && (
                    <div className="absolute left-0 right-0 top-[calc(100%+0.35rem)] z-[80] rounded-2xl border-2 border-border/70 bg-card p-1.5 shadow-2xl shadow-black/10 animate-in fade-in zoom-in-95 duration-150">
                      {memberFilterOptions.map(option => (
                        <button
                          key={option.value}
                          type="button"
                          onClick={() => { setViewFilter(option.value); setFilterOpen(false); }}
                          className={cn(
                            "w-full rounded-xl px-3 py-2 text-left transition-all flex items-center justify-between gap-3",
                            viewFilter === option.value ? "bg-primary/10 text-primary" : "hover:bg-muted/60 text-foreground"
                          )}
                        >
                          <span className="text-[11px] font-black">{option.label}</span>
                          <span className="flex items-center gap-2">
                            <span className="text-[9px] font-black text-muted-foreground">{option.count} 项</span>
                            {viewFilter === option.value && <CheckCircle2 className="size-3.5" />}
                          </span>
                        </button>
                      ))}
                    </div>
                  )}
                </div>
                <div className="relative flex-1">
                  <Search className="absolute left-3 top-1/2 -translate-y-1/2 size-3.5 text-muted-foreground pointer-events-none" />
                  <input
                    value={searchQuery}
                    onChange={(event) => setSearchQuery(event.target.value)}
                    placeholder="搜索节点、订阅组、国家、协议..."
                    className="w-full h-11 rounded-xl border-2 border-muted-foreground/10 bg-background pl-9 pr-9 text-[12px] font-bold outline-none shadow-inner transition-all placeholder:text-muted-foreground/35 focus:border-primary/40 focus:ring-4 focus:ring-primary/10"
                  />
                  {searchQuery && (
                    <button
                      type="button"
                      onClick={() => setSearchQuery('')}
                      className="absolute right-2.5 top-1/2 -translate-y-1/2 rounded-lg p-1 text-muted-foreground hover:bg-muted hover:text-foreground transition-colors"
                    >
                      <X className="size-3.5" />
                    </button>
                  )}
                </div>
              </div>
              <div className="space-y-8">
                {showImported && <AssetSection title="已导入" subtitle="当前实际入池并参与运行配置的节点与订阅组" section={importedSection} />}
                {showRejected && <AssetSection title="已剔除" subtitle="被精选规则剔除的原始节点" section={rejectedSection} rejected />}
              </div>
            </div>
          ) : (
            <div className="h-full min-h-[50vh] flex items-center justify-center text-[10px] font-black text-muted-foreground">成员数据加载失败</div>
          )}
        </div>
      </div>
    </div>
  );
};

export const Subscriptions = () => {
  const { toast } = useToast();
  const [subs, setSubs] = useState<Subscription[]>([]);
  const [loading, setLoading] = useState(true);
  const [editingSub, setEditingSub] = useState<SubscriptionDraft | null>(null);
  const [membersSub, setMembersSub] = useState<Subscription | null>(null);
  const [membersData, setMembersData] = useState<SubscriptionMembers | null>(null);
  const [membersLoading, setMembersLoading] = useState(false);
  const [selectionMembers, setSelectionMembers] = useState<SubscriptionMembers | null>(null);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [isGlobalDrawerOpen, setIsGlobalDrawerOpen] = useState(false);
  const [globalRules, setGlobalRules] = useState<FilterRule[]>([]);
  const [isSavingGlobalRules, setIsSavingGlobalRules] = useState(false);
  const [isSavingSubscription, setIsSavingSubscription] = useState(false);
  const subscriptionSubmitInFlight = useRef(false);
  const globalRulesSubmitInFlight = useRef(false);
  const membersRequestId = useRef(0);

  const fetchSubs = useCallback(async () => {
    try {
      const data = await api.listSubscriptions();
      setSubs(data);
    } catch {
      toast('订阅列表加载失败', 'error');
    } finally {
      setLoading(false);
    }
  }, [toast]);

  const fetchGlobalRules = useCallback(async () => {
    try {
      setGlobalRules(await api.listGlobalFilterRules());
    } catch {
      toast('通用精选准则加载失败', 'error');
    }
  }, [toast]);

  useEffect(() => {
    queueMicrotask(() => {
      void fetchSubs();
      void fetchGlobalRules();
    });
  }, [fetchGlobalRules, fetchSubs]);

  const openEditor = (draft: SubscriptionDraft) => {
    setSelectionMembers(null);
    setShowAdvanced(draft.rules.length > 0);
    setEditingSub(draft);
  };

  useEffect(() => {
    if (!editingSub?.id) return;
    let cancelled = false;
    api.subscriptionMembers(editingSub.id)
      .then(data => { if (!cancelled) setSelectionMembers(data); })
      .catch(() => { if (!cancelled) setSelectionMembers(null); });
    return () => { cancelled = true; };
  }, [editingSub?.id]);

  const selectionNodeOptions = useMemo<SelectOption[]>(() => {
    const nodes = selectionMembers?.beforeFilter.nodes ?? [];
    return nodes.map(node => ({
      value: node.displayName || displayAssetName(node.name),
      label: node.displayName || displayAssetName(node.name),
      meta: `${node.country || 'UNK'} · ${node.protocol}${node.filteredOut ? ' · 已剔除' : ''}`,
    }));
  }, [selectionMembers]);

  const handleUpdateSub = async () => {
    if (!editingSub || subscriptionSubmitInFlight.current) return;
    // 1. Validate URL
    if (!editingSub.url.trim()) {
      toast('节点接口地址不能为空', 'error');
      return;
    }
    try {
      new URL(editingSub.url);
    } catch {
      toast('节点接口地址格式不正确，请包含 http/https', 'error');
      return;
    }

    // 2. Validate Rules
    if (editingSub.rules && editingSub.rules.length > 0) {
      for (let i = 0; i < editingSub.rules.length; i++) {
        const rule = editingSub.rules[i];
        if (rule.type === 'in') {
          if (!Array.isArray(rule.values) || rule.values.length === 0) {
            toast(`规则 #${i + 1} 的精确节点不能为空`, 'error');
            return;
          }
        } else if (!rule.pattern.trim()) {
          toast(`规则 #${i + 1} 的关键字/匹配模式不能为空`, 'error');
          return;
        }
        if (rule.type === 'regex') {
          try {
            new RegExp(rule.pattern);
          } catch {
            toast(`规则 #${i + 1} 的正则表达式不合法`, 'error');
            return;
          }
        }
      }
    }

    const payload: SubscriptionInput = {
      name: editingSub.name,
      url: editingSub.url,
      format: editingSub.format,
      intervalSeconds: editingSub.intervalSeconds,
      inheritGlobal: editingSub.inheritGlobal,
      rules: editingSub.rules.map(rule => ({
        action: rule.action,
        type: rule.type,
        pattern: rule.type === 'in' ? '' : rule.pattern,
        values: rule.type === 'in' ? (rule.values ?? []) : [],
        enabled: rule.enabled ?? true,
      })),
      downloadRoute: editingSub.downloadRoute,
    };

    subscriptionSubmitInFlight.current = true;
    setIsSavingSubscription(true);
    try {
      const nextSubs = editingSub.id
        ? await api.updateSubscription(editingSub.id, payload)
        : await api.createSubscription(payload);
      setSubs(nextSubs);
      toast('配置已同步', 'success');
      setEditingSub(null);
    } catch {
      toast('配置同步失败', 'error');
    } finally {
      subscriptionSubmitInFlight.current = false;
      setIsSavingSubscription(false);
    }
  };

  const handleDelete = async (id: string) => {
    if (confirm('确认移除该资源？')) {
      try {
        await api.deleteSubscription(id);
        toast('资源已移除', 'success');
        void fetchSubs();
      } catch {
        toast('移除失败', 'error');
      }
    }
  };

  const handleViewMembers = async (sub: Subscription) => {
    const requestId = ++membersRequestId.current;
    setMembersSub(sub);
    setMembersData(null);
    setMembersLoading(true);
    try {
      const data = await api.subscriptionMembers(sub.id);
      if (membersRequestId.current === requestId) setMembersData(data);
    } catch {
      if (membersRequestId.current === requestId) toast('订阅成员加载失败', 'error');
    } finally {
      if (membersRequestId.current === requestId) setMembersLoading(false);
    }
  };

  const normalizeSubForEdit = (sub: Subscription): SubscriptionDraft => ({
    ...sub,
    rules: sub.rules.map(rule => ({
      ...rule,
      pattern: rule.pattern ?? '',
      values: Array.isArray(rule.values)
        ? rule.values
        : rule.type === 'in' && rule.pattern
          ? String(rule.pattern).split(',').map(item => item.trim()).filter(Boolean)
          : [],
    })),
  });

  const handleSaveGlobalRules = async () => {
    if (globalRulesSubmitInFlight.current) return;
    for (let i = 0; i < globalRules.length; i++) {
      const rule = globalRules[i];
      if (rule.type === 'in' ? !(rule.values?.length) : !rule.pattern.trim()) {
        toast(`通用准则 #${i + 1} 的匹配模式不能为空`, 'error');
        return;
      }
      if (rule.type === 'regex') {
        try {
          new RegExp(rule.pattern);
        } catch {
          toast(`通用准则 #${i + 1} 的正则表达式不合法`, 'error');
          return;
        }
      }
    }

    const payload: FilterRuleInput[] = globalRules.map(rule => ({
      action: rule.action,
      type: rule.type,
      pattern: rule.type === 'in' ? '' : rule.pattern,
      values: rule.type === 'in' ? (rule.values ?? []) : [],
      enabled: rule.enabled,
    }));

    globalRulesSubmitInFlight.current = true;
    setIsSavingGlobalRules(true);
    try {
      setGlobalRules(await api.replaceGlobalFilterRules(payload));
      setIsGlobalDrawerOpen(false);
      toast('通用精选准则已应用', 'success');
    } catch {
      toast('通用精选准则保存失败', 'error');
    } finally {
      globalRulesSubmitInFlight.current = false;
      setIsSavingGlobalRules(false);
    }
  };

  const updateGlobalRule = (id: string, updates: Partial<FilterRule>) => {
    setGlobalRules(rules => rules.map(rule => rule.id === id ? { ...rule, ...updates } : rule));
  };

  const intervals = [
    { label: '6H', value: 360 }, { label: '12H', value: 720 }, { label: '24H', value: 1440 }, { label: 'NEVER', value: 0 },
  ];

  const [lastAddedIndex, setLastAddedIndex] = useState<number | null>(null);

  const handleAddRule = () => {
    if (!editingSub) return;
    const newRule: EditableFilterRule = { pattern: '', values: [], action: 'keep', type: 'contains', enabled: true };
    const newRules = [...editingSub.rules, newRule];
    setEditingSub({ ...editingSub, rules: newRules });
    setLastAddedIndex(newRules.length - 1);
  };

  const handleDeleteRule = (index: number) => {
    if (!editingSub) return;
    const newRules = [...editingSub.rules];
    newRules.splice(index, 1);
    setEditingSub({ ...editingSub, rules: newRules });
    setLastAddedIndex(null);
  };

  const handleUpdateRule = (index: number, updates: Partial<EditableFilterRule>) => {
    if (!editingSub) return;
    const newRules = [...editingSub.rules];
    newRules[index] = { ...newRules[index], ...updates };
    setEditingSub({ ...editingSub, rules: newRules });
  };

  const ruleTypes = [
    { value: 'contains', label: '包含' },
    { value: 'in', label: '等于' },
    { value: 'not_contains', label: '不包含' },
    { value: 'regex', label: '正则匹配' }
  ];

  const handleCycleRuleType = (index: number) => {
    if (!editingSub) return;
    const currentType = editingSub.rules[index].type || 'contains';
    const currentIndex = ruleTypes.findIndex(t => t.value === currentType);
    const nextType = ruleTypes[(currentIndex + 1) % ruleTypes.length].value;
    handleUpdateRule(index, {
      type: nextType,
      pattern: nextType === 'in' ? '' : editingSub.rules[index].pattern,
      values: nextType === 'in' ? (editingSub.rules[index].values ?? []) : [],
    });
  };

  const ruleTypeLabel = (type: string) => ruleTypes.find(item => item.value === type)?.label ?? '包含';

  if (loading) return <div className="flex items-center justify-center h-[60vh]"><Loader2 className="animate-spin text-primary size-8" /></div>;

  return (
    <div className="space-y-6 md:space-y-10 max-w-7xl mx-auto pb-20 animate-in fade-in duration-500 text-left">
      {/* Header */}
      <div className="flex justify-between items-center text-left px-2">
        <div>
          <h2 className="text-2xl md:text-3xl font-black uppercase tracking-tight text-left">订阅资源池</h2>
          <div className="flex items-center gap-2 mt-1 hidden sm:flex">
             <Info className="size-3 text-muted-foreground" />
             <p className="text-[10px] font-bold text-muted-foreground uppercase tracking-wider">Smart Merge & Selection</p>
          </div>
        </div>
        <Button onClick={() => openEditor({ ...EMPTY_SUBSCRIPTION_DRAFT, rules: [] })} className="rounded-xl md:rounded-2xl gap-2 shadow-xl shadow-primary/20 font-black text-[9px] md:text-[10px] h-10 md:h-12 px-6 md:px-10 uppercase transition-all hover:scale-105 active:scale-95">
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
           <p className="text-[10px] font-black text-primary uppercase tracking-wide truncate flex-1">
             {globalRules.length} 条准则 · 开启继承后自动精简节点库
           </p>
         </div>
         <div className="flex items-center gap-1.5 md:gap-2 relative z-10 text-primary shrink-0 bg-primary/5 px-3 py-1.5 rounded-xl border border-primary/10 group-hover:bg-primary group-hover:text-white transition-all shadow-sm">
            <span className="text-[10px] font-black uppercase tracking-wide">配置准则</span>
            <ChevronRight className="size-3 md:size-4" />
         </div>
      </div>

      {/* Grid */}
      <div className="space-y-4 md:space-y-6 text-left px-2">
        <div className="flex items-center gap-4 text-muted-foreground"><Layers className="size-4" /><h3 className="text-[10px] font-black uppercase tracking-wider text-left">资源矩阵</h3><div className="h-px flex-1 bg-muted" /></div>
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4 md:gap-6 text-left">
          {subs.map(sub => (
            <SubscriptionCard key={sub.id} sub={sub} onEdit={(item: Subscription) => openEditor(normalizeSubForEdit(item))} onDelete={handleDelete} onMembers={handleViewMembers} />
          ))}
          <button onClick={() => openEditor({ ...EMPTY_SUBSCRIPTION_DRAFT, rules: [] })} className="border-2 border-dashed rounded-[1.5rem] md:rounded-[2rem] flex flex-col items-center justify-center p-6 md:p-10 space-y-3 hover:bg-primary/[0.02] hover:border-primary/50 transition-all group min-h-[160px] md:min-h-[200px]">
            <Plus className="size-6 md:size-8 text-muted-foreground group-hover:scale-110 transition-transform" />
            <span className="text-[10px] font-black text-muted-foreground uppercase tracking-wide">Connect New</span>
          </button>
        </div>
      </div>

      {membersSub && <MembersDrawer key={membersSub.id} sub={membersSub} data={membersData} loading={membersLoading} onClose={() => { membersRequestId.current += 1; setMembersSub(null); setMembersData(null); setMembersLoading(false); }} />}

      {/* Editor Side Panel (Drawer) - RESPONSIBLE REDESIGN */}
      {editingSub && (
        <div className="fixed inset-0 z-50 flex justify-end overflow-hidden">
          <div className="absolute inset-0 bg-background/60 backdrop-blur-md" onClick={() => setEditingSub(null)} />
          <div className="relative w-full sm:max-w-md md:max-w-xl bg-card border-l h-full shadow-2xl flex flex-col animate-in slide-in-from-right duration-500">
            <div className="p-4 md:p-5 border-b flex justify-between items-center bg-muted/20">
              <div className="flex items-center gap-3">
                <div className="size-9 md:size-10 rounded-xl bg-primary text-primary-foreground flex items-center justify-center shadow-lg"><Activity className="size-5" /></div>
                <div><h3 className="text-base md:text-lg font-black uppercase tracking-tight">资源资产配置</h3><p className="text-[10px] font-black text-primary uppercase tracking-wide">Resource Management</p></div>
              </div>
              <Button variant="ghost" size="icon" onClick={() => setEditingSub(null)} className="rounded-xl size-9 md:size-10 hover:bg-muted"><X className="size-5" /></Button>
            </div>
            
            <div className="flex-1 overflow-y-auto p-4 md:p-6 space-y-6 md:space-y-8 custom-scrollbar">
              {/* Basic Section - COMPACT */}
              <section className="space-y-4">
                <div className="flex items-center gap-2.5"><div className="h-6 w-1.5 bg-primary rounded-full" /><h4 className="text-sm md:text-base font-black uppercase tracking-tight">基础接入配置</h4></div>
                <div className="grid grid-cols-1 gap-4 bg-muted/30 p-4 md:p-6 rounded-2xl border border-muted shadow-inner">
                  <div className="space-y-1.5">
                    <label className="text-[10px] font-black uppercase ml-1 text-muted-foreground block tracking-wide">资源标识名称</label>
                    <input value={editingSub.name} onChange={e => setEditingSub({...editingSub, name: e.target.value})} placeholder="例如：飞机场主线" className="w-full bg-background border-2 border-transparent focus:border-primary/40 rounded-xl px-4 py-3 font-black outline-none transition-all shadow-sm text-sm md:text-base" />
                  </div>
                  <div className="space-y-1.5">
                    <label className="text-[10px] font-black uppercase ml-1 text-muted-foreground block tracking-wide">节点接口地址 (URL)</label>
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
                        editingSub.inheritGlobal ? "bg-primary/[0.03] border-primary/20 shadow-sm" : "bg-muted/30 border-transparent grayscale opacity-80"
                      )}
                    >
                       <div className="flex items-center gap-3">
                         <div className={cn("size-8 md:size-10 rounded-lg md:rounded-xl flex items-center justify-center shadow-lg transition-all", editingSub.inheritGlobal ? "bg-primary text-white shadow-primary/20" : "bg-zinc-500 text-white")}><Shield className="size-4 md:size-5" /></div>
                         <div className="text-left">
                           <p className={cn("font-black uppercase tracking-tight text-[10px] md:text-xs", editingSub.inheritGlobal ? "text-primary" : "text-zinc-600")}>继承通用精选准则</p>
                           <p className="text-[10px] font-bold text-muted-foreground uppercase tracking-wide">Common Standards</p>
                         </div>
                       </div>
                       <div className={cn("w-10 md:w-12 h-5 md:h-6 rounded-full relative transition-all shadow-inner border border-black/5", editingSub.inheritGlobal ? "bg-primary" : "bg-zinc-400")}>
                          <div className={cn("absolute top-1 size-3 md:size-4 bg-white rounded-full transition-all shadow-md", editingSub.inheritGlobal ? "right-1" : "left-1")} />
                       </div>
                    </button>
                  </div>

                  <div className="space-y-2.5 pt-2 border-t border-dashed border-muted">
                    <label className="text-[10px] font-black uppercase ml-1 text-muted-foreground block tracking-wide">自动同步频率</label>
                    <div className="flex flex-wrap gap-1.5">
                       {intervals.map((item) => (
                         <button key={item.value} onClick={() => setEditingSub({...editingSub, intervalSeconds: item.value * 60})}
                           className={cn("px-3 md:px-4 py-1.5 md:py-2 rounded-lg md:rounded-xl text-[10px] font-black uppercase border-2 transition-all active:scale-95",
                             editingSub.intervalSeconds === item.value * 60 ? "bg-zinc-900 text-white border-zinc-900 shadow-lg" : "bg-background border-transparent text-muted-foreground hover:bg-muted")}>{item.label}</button>
                       ))}
                    </div>
                  </div>
                  <div className="space-y-2.5 pt-2 border-t border-dashed border-muted">
                    <label className="text-[10px] font-black uppercase ml-1 text-muted-foreground block tracking-wide">下载路线</label>
                    <Select value={editingSub.downloadRoute} onValueChange={value => setEditingSub({ ...editingSub, downloadRoute: value as DownloadRoute })}>
                      <SelectTrigger aria-label="订阅下载路线"><SelectValue /></SelectTrigger>
                      <SelectContent>
                        <SelectGroup>
                          {(Object.entries(DOWNLOAD_ROUTE_LABELS) as Array<[DownloadRoute, string]>).map(([value, label]) => (
                            <SelectItem key={value} value={value}>{label}</SelectItem>
                          ))}
                        </SelectGroup>
                      </SelectContent>
                    </Select>
                  </div>
                </div>
              </section>

              {/* ASSET COMPOSITION - RESPONSIVE TILES */}
              {editingSub.id && editingSub.breakdown && (
                <section className="space-y-4">
                  <div className="flex items-center gap-2.5"><div className="h-6 w-1.5 bg-primary/40 rounded-full" /><h4 className="text-sm md:text-base font-black uppercase tracking-tight">入池资产透视</h4></div>
                  <div className="grid grid-cols-2 md:grid-cols-3 gap-3">
                    {Object.entries(editingSub.breakdown).map(([type, count]) => (
                      <div key={type} className="bg-muted/30 border-2 rounded-xl md:rounded-2xl p-3 md:p-4 shadow-sm flex flex-col items-start gap-0.5 group hover:border-primary/20 transition-all border-b-4 border-b-primary/5">
                         <span className="text-[10px] font-black text-primary uppercase tracking-wide">{type}</span>
                         <div className="flex items-baseline gap-1"><p className="text-xl md:text-2xl font-black tracking-tighter">{count}</p><span className="text-[10px] font-bold text-muted-foreground uppercase tracking-wide">PCS</span></div>
                      </div>
                    ))}
                  </div>
                </section>
              )}

              {/* SELECTION RULES - COMPACT BLOCKS */}
              <section className="space-y-4">
                {!showAdvanced ? (
                  <button onClick={() => setShowAdvanced(true)} className="group flex items-center gap-3 text-[10px] font-black uppercase text-primary bg-primary/5 px-6 py-3.5 rounded-xl md:rounded-2xl border-2 border-primary/10 shadow-sm transition-all hover:bg-primary/10 w-full justify-center"><Filter className="size-3.5 animate-bounce" /> 配置个体精选规则 (Advanced Rules) <ChevronDown className="size-3" /></button>
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
                         <span className="text-[10px] font-bold text-green-700 uppercase tracking-tight mt-1 animate-pulse">Build your scheme</span>
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
                            点击 <span className="bg-primary/10 px-1 rounded font-bold text-primary">引入/剔除</span> 切换逻辑，点击 <span className="bg-muted px-1 rounded font-bold">包含/等于/正则</span> 切换模式。
                          </p>
                       </div>
                    </div>

                    <div className="space-y-3">
                      {editingSub.rules.length === 0 && (
                        <div className="py-10 border-2 border-dashed border-muted rounded-2xl flex flex-col items-center justify-center text-center space-y-3 opacity-80">
                           <Layers className="size-8 mb-1" />
                           <div>
                             <p className="text-xs font-black uppercase tracking-widest">暂无活跃准则</p>
                             <p className="text-[9px] font-bold">点击上方按钮，开启个性化精选</p>
                           </div>
                        </div>
                      )}
                      {editingSub.rules.map((rule, i) => (
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

                            <div className="flex-1 flex items-center gap-2 min-w-0">
                               <span className="text-[10px] font-bold text-muted-foreground shrink-0">名字方案</span>
                               
                               {/* Match Type Dropdown-style Button */}
                               <button 
                                 onClick={() => handleCycleRuleType(i)}
                                 className={cn(
                                   "flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg border-2 text-[10px] font-black transition-all hover:bg-muted active:scale-95 shrink-0",
                                   rule.type === 'regex' ? "bg-zinc-900 border-zinc-800 text-emerald-400 font-mono shadow-inner" : 
                                   rule.type === 'in' ? "bg-primary/10 border-primary/20 text-primary shadow-sm" :
                                   rule.type === 'not_contains' ? "bg-amber-50 border-amber-200 text-amber-600 shadow-sm" : 
                                   "bg-background border-muted text-muted-foreground shadow-sm"
                                 )}
                               >
                                 {rule.type === 'regex' && <span className="opacity-80 text-[10px]">.*</span>}
                                 {rule.type === 'not_contains' && <ShieldAlert className="size-3" />}
                                 {ruleTypeLabel(rule.type)}
                                 <ChevronDown className="size-2.5 opacity-70 ml-0.5" />
                               </button>

                               {/* Pattern Input - PROMINENT SLOT DESIGN */}
                               {rule.type === 'in' ? (
                                 <RuleMultiSelect
                                   values={rule.values ?? []}
                                   placeholder={selectionNodeOptions.length > 0 ? '选择精确节点...' : '先同步订阅后选择...'}
                                   options={selectionNodeOptions}
                                   onChange={(values) => handleUpdateRule(i, { values, pattern: '' })}
                                 />
                               ) : (
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
                               )}
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
              <Button variant="outline" disabled={isSavingSubscription} onClick={() => setEditingSub(null)} className="rounded-xl md:rounded-2xl h-12 md:h-14 font-black uppercase border-2 text-[9px] md:text-xs tracking-widest hover:bg-background transition-all">放弃修改</Button>
              <Button disabled={isSavingSubscription} onClick={handleUpdateSub} className="rounded-xl md:rounded-2xl h-12 md:h-14 bg-zinc-900 hover:bg-black text-white font-black uppercase shadow-xl shadow-black/20 text-[9px] md:text-xs tracking-widest transition-all hover:scale-105 active:scale-95">
                {isSavingSubscription && <Spinner data-icon="inline-start" />}
                {isSavingSubscription ? '正在同步' : '保存配置同步'}
              </Button>
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
                 <div className="flex justify-between items-center">
                   <span className="text-[10px] font-black uppercase text-muted-foreground tracking-widest ml-1">有效准则</span>
                   <Button
                     variant="outline"
                     size="sm"
                     onClick={() => setGlobalRules(rules => [...rules, { id: `draft-${Date.now()}`, action: 'discard', pattern: '', type: 'contains', enabled: true }])}
                     className="rounded-lg font-black text-[10px] uppercase border-2 h-8"
                   >
                     添加
                   </Button>
                 </div>
                 {globalRules.map(rule => (
                    <div key={rule.id} className="bg-background border-2 border-muted rounded-xl p-4 shadow-sm group border-l-4 border-l-red-500">
                       <div className="flex items-center gap-4">
                         <button
                           onClick={() => updateGlobalRule(rule.id, { action: rule.action === 'keep' ? 'discard' : 'keep' })}
                           className={cn("size-8 md:size-10 rounded-lg md:rounded-xl text-white flex items-center justify-center shrink-0", rule.action === 'keep' ? "bg-emerald-500" : "bg-red-500")}
                         >
                           {rule.action === 'keep' ? <CheckCircle2 className="size-4 md:size-5" /> : <ZapOff className="size-4 md:size-5" />}
                         </button>
                         <button
                           onClick={() => updateGlobalRule(rule.id, { type: rule.type === 'contains' ? 'regex' : 'contains' })}
                           className="px-2.5 py-1.5 rounded-lg border-2 text-[10px] font-black uppercase bg-muted/40 shrink-0"
                         >
                           {rule.type === 'regex' ? '正则' : '包含'}
                         </button>
                         <input
                           value={rule.pattern}
                           onChange={(event) => updateGlobalRule(rule.id, { pattern: event.target.value })}
                           placeholder="名字方案匹配模式..."
                           className="flex-1 min-w-0 bg-muted/30 border-2 border-transparent focus:border-primary/30 rounded-xl px-3 py-2 text-xs font-bold outline-none"
                         />
                         <button onClick={() => setGlobalRules(rules => rules.filter(item => item.id !== rule.id))} className="opacity-0 group-hover:opacity-100 p-2 text-destructive transition-all"><Trash2 className="size-4" /></button>
                       </div>
                    </div>
                 ))}
              </div>
            </div>
            <div className="p-6 border-t bg-muted/10">
              <Button onClick={handleSaveGlobalRules} disabled={isSavingGlobalRules} className="w-full h-14 rounded-xl font-black uppercase shadow-xl shadow-primary/30 tracking-widest transition-all hover:scale-[1.02] active:scale-95">
                {isSavingGlobalRules ? <Loader2 className="size-4 animate-spin" /> : null}
                保存并应用
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
