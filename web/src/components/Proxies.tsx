import { useState, useEffect } from 'react';
import { 
  Search, 
  LayoutGrid, 
  Zap, 
  Globe, 
  Plus, 
  Loader2,
  ChevronRight,
  LayoutList
} from 'lucide-react';
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { useToast } from './Toast';

const NodeItem = ({ node, isSelected, isSwitching, onSelect }: any) => (
  <div 
    onClick={() => !isSwitching && onSelect(node.name)}
    className={cn(
      "bg-card border rounded-xl p-3 flex items-center justify-between hover:border-primary transition-all cursor-pointer group relative overflow-hidden shadow-sm",
      isSelected ? "border-primary bg-primary/[0.03] ring-1 ring-primary/10" : "hover:bg-muted/50",
      isSwitching && isSelected && "opacity-70"
    )}
  >
    <div className="flex items-center gap-3 min-w-0">
      {isSwitching && isSelected ? (
        <Loader2 className="size-3 animate-spin text-primary" />
      ) : (
        <div className={cn("size-2 rounded-full", isSelected ? "bg-primary animate-pulse" : "bg-muted-foreground/30")} />
      )}
      <div className="flex flex-col min-w-0 text-left">
        <span className={cn("text-sm font-bold truncate", isSelected && "text-primary")}>{node.name.split('@')[0]}</span>
        <span className="text-[9px] font-black text-muted-foreground uppercase tracking-tighter">
          {node.type} · <span className="text-primary/70">@{node.name.split('@')[1]}</span>
        </span>
      </div>
    </div>
    <span className={cn(
      "text-[10px] font-mono font-black",
      node.latency < 100 ? "text-green-500" : node.latency < 300 ? "text-yellow-500" : "text-red-500"
    )}>
      {node.latency}ms
    </span>
  </div>
);

const GroupCard = ({ name, now, type, all, delay, active, onClick }: any) => (
  <div 
    onClick={onClick}
    className={cn(
      "bg-card border rounded-2xl p-5 hover:shadow-lg transition-all cursor-pointer group relative overflow-hidden",
      active ? "border-primary ring-2 ring-primary/10 bg-primary/[0.02]" : "hover:border-primary/50"
    )}
  >
    <div className="flex justify-between items-start mb-4">
      <div className="space-y-1 text-left">
        <h4 className="font-black text-lg tracking-tight">{name}</h4>
        <p className="text-[10px] text-muted-foreground uppercase font-bold tracking-widest flex items-center gap-1">
          <span className="bg-muted px-1.5 py-0.5 rounded">{type}</span> · {all.length} 成员
        </p>
      </div>
      <div className={cn(
        "px-2 py-1 rounded-lg text-[10px] font-black font-mono shadow-sm border",
        delay < 100 ? "bg-green-500/10 text-green-500 border-green-500/20" : 
        delay < 300 ? "bg-yellow-500/10 text-yellow-500 border-yellow-500/20" : 
        "bg-red-500/10 text-red-500 border-red-500/20"
      )}>
        {delay}ms
      </div>
    </div>
    
    <div className="flex items-center gap-3 mt-4 pt-4 border-t border-dashed">
      <div className="size-8 rounded-xl bg-muted flex items-center justify-center font-black text-[10px] shrink-0 text-muted-foreground">
        {now.split('@')[0].substring(0, 2).toUpperCase()}
      </div>
      <div className="flex-1 min-w-0 text-left">
        <p className="text-xs font-black truncate">{now.split('@')[0]}</p>
        <p className="text-[9px] text-primary font-bold">@{now.split('@')[1] || 'SYSTEM'}</p>
      </div>
      <ChevronRight className={cn("size-4 text-muted-foreground transition-transform", active && "rotate-90 text-primary")} />
    </div>
  </div>
);

export const Proxies = () => {
  const { toast } = useToast();
  const [view, setView] = useState<'groups' | 'all'>('groups');
  const [groups, setGroups] = useState<any[]>([]);
  const [nodes, setNodes] = useState<any[]>([]);
  const [activeGroup, setActiveGroup] = useState<string | null>(null);
  const [isSwitching, setIsSwitching] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [searchQuery, setSearchQuery] = useState('');
  const [isCountryGrouped, setIsCountryGrouped] = useState(true);

  const countryFlags: any = { 'HK': '🇭🇰', 'TW': '🇹🇼', 'JP': '🇯🇵', 'US': '🇺🇸', 'SG': '🇸🇬', 'CN': '🇨🇳', 'DEFAULT': '🏳️' };
  const countryNames: any = { 'HK': '香港 (HK)', 'TW': '台湾 (TW)', 'JP': '日本 (JP)', 'US': '美国 (US)', 'SG': '新加坡 (SG)', 'CN': '中国 (CN)', 'DEFAULT': '未知国家' };

  const fetchData = async () => {
    try {
      const res = await fetch('/api/proxies');
      const data = await res.json();
      setGroups(data.groups);
      setNodes(data.nodes);
      if (!activeGroup && data.groups.length > 0) setActiveGroup(data.groups[0].name);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchData();
  }, []);

  const handleSelectNode = async (groupName: string, nodeName: string) => {
    setIsSwitching(nodeName);
    try {
      await fetch(`/api/proxies/${groupName}`, {
        method: 'PUT',
        body: JSON.stringify({ name: nodeName })
      });
      toast(`已切换至: ${nodeName.split('@')[0]}`, 'success');
      await fetchData();
    } catch (e) {
      toast('切换失败', 'error');
    } finally {
      setIsSwitching(null);
    }
  };

  const selectedGroupData = groups.find(g => g.name === activeGroup);

  const renderNodeList = (memberNames: string[]) => {
    const filteredNodes = nodes
      .filter(n => memberNames.includes(n.name))
      .filter(n => n.name.toLowerCase().includes(searchQuery.toLowerCase()));

    if (isCountryGrouped) {
      const grouped: any = {};
      filteredNodes.forEach(n => {
        const country = n.country || 'DEFAULT';
        if (!grouped[country]) grouped[country] = [];
        grouped[country].push(n);
      });

      // 特殊逻辑：优先显示香港和台湾
      const sortedKeys = Object.keys(grouped).sort((a, b) => {
        if (a === 'HK') return -1;
        if (b === 'HK') return 1;
        if (a === 'TW') return -1;
        if (b === 'TW') return 1;
        return a.localeCompare(b);
      });

      return sortedKeys.map(code => (
        <div key={code} className="space-y-4 mb-10 last:mb-0">
          <div className="flex items-center gap-3">
            <span className="text-xl">{countryFlags[code] || countryFlags.DEFAULT}</span>
            <h4 className="font-black text-xs uppercase tracking-[0.2em] text-muted-foreground">{countryNames[code] || countryNames.DEFAULT}</h4>
            <span className="text-[10px] bg-muted px-2 py-0.5 rounded-md font-black">{grouped[code].length}</span>
            <div className="flex-1 h-px bg-gradient-to-r from-muted to-transparent ml-2" />
          </div>
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
            {grouped[code].map((node: any) => (
              <NodeItem 
                key={node.name}
                node={node}
                isSelected={selectedGroupData?.now === node.name}
                isSwitching={isSwitching === node.name}
                onSelect={(name: string) => handleSelectNode(selectedGroupData!.name, name)}
              />
            ))}
          </div>
        </div>
      ));
    }

    return (
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
        {filteredNodes.map((node) => (
          <NodeItem 
            key={node.name}
            node={node}
            isSelected={selectedGroupData?.now === node.name}
            isSwitching={isSwitching === node.name}
            onSelect={(name: string) => handleSelectNode(selectedGroupData!.name, name)}
          />
        ))}
      </div>
    );
  };

  if (loading) return <div className="flex items-center justify-center h-[60vh]"><Loader2 className="animate-spin text-primary" /></div>;

  return (
    <div className="space-y-8 max-w-7xl mx-auto pb-20 animate-in fade-in duration-500">
      {/* Header */}
      <div className="flex flex-col md:flex-row justify-between items-center gap-4">
        <div className="flex items-center gap-2 bg-muted/50 p-1 rounded-2xl border">
          <Button variant={view === 'groups' ? 'default' : 'ghost'} size="sm" onClick={() => setView('groups')} className="rounded-xl font-black text-[10px] uppercase">
            <LayoutGrid className="size-3.5 mr-2" /> 策略组
          </Button>
          <Button variant={view === 'all' ? 'default' : 'ghost'} size="sm" onClick={() => setView('all')} className="rounded-xl font-black text-[10px] uppercase">
            <Globe className="size-3.5 mr-2" /> 所有节点
          </Button>
        </div>
        <Button onClick={() => toast('新增策略组面板正在开发中...', 'info')} className="rounded-xl gap-2 font-black text-xs uppercase shadow-lg shadow-primary/20">
          <Plus className="size-4" /> 新增策略组
        </Button>
      </div>

      {view === 'groups' ? (
        <div className="space-y-10">
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6">
            {groups.map((group) => (
              <GroupCard key={group.name} {...group} active={activeGroup === group.name} onClick={() => setActiveGroup(group.name)} />
            ))}
          </div>

          {selectedGroupData && (
            <div className="bg-card border rounded-[2.5rem] overflow-hidden shadow-2xl shadow-primary/5">
              <div className="p-8 border-b bg-muted/10 flex flex-col md:flex-row justify-between gap-6">
                <div className="flex items-center gap-6">
                  <div className="size-16 rounded-[1.25rem] bg-primary text-primary-foreground flex items-center justify-center font-black text-2xl shadow-xl shadow-primary/20">
                    {selectedGroupData.name.substring(0, 2).toUpperCase()}
                  </div>
                  <div className="text-left">
                    <h3 className="font-black text-2xl tracking-tight">{selectedGroupData.name}</h3>
                    <p className="text-xs font-bold text-muted-foreground mt-1 uppercase tracking-widest">
                      {selectedGroupData.type} · {selectedGroupData.all.length} 成员
                    </p>
                  </div>
                </div>

                <div className="flex items-center gap-3">
                  <div className="relative">
                    <Search className="absolute left-4 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
                    <input 
                      value={searchQuery}
                      onChange={(e) => setSearchQuery(e.target.value)}
                      placeholder="在组内搜索..." 
                      className="pl-11 pr-4 py-2.5 bg-background border rounded-2xl text-sm font-bold focus:ring-2 ring-primary/20 outline-none w-48 md:w-64 transition-all"
                    />
                  </div>
                  <Button 
                    variant="outline" 
                    onClick={() => setIsCountryGrouped(!isCountryGrouped)}
                    className={cn("rounded-2xl size-11 p-0", isCountryGrouped && "bg-primary/5 border-primary text-primary")}
                  >
                    {isCountryGrouped ? <LayoutList className="size-5" /> : <LayoutGrid className="size-5" />}
                  </Button>
                  <Button variant="outline" onClick={() => toast('开始组内全量测速...', 'info')} className="rounded-2xl size-11 p-0">
                    <Zap className="size-5 text-yellow-500" />
                  </Button>
                </div>
              </div>
              
              <div className="p-8 min-h-[300px]">
                {renderNodeList(selectedGroupData.all)}
              </div>
            </div>
          )}
        </div>
      ) : (
        <div className="space-y-8">
           <div className="relative max-w-md mx-auto">
             <Search className="absolute left-4 top-1/2 -translate-y-1/2 size-5 text-muted-foreground" />
             <input 
               value={searchQuery}
               onChange={(e) => setSearchQuery(e.target.value)}
               placeholder="全局搜索所有节点..." 
               className="w-full pl-12 pr-4 py-4 bg-card border rounded-[1.5rem] text-lg font-black focus:ring-4 ring-primary/10 outline-none shadow-xl"
             />
           </div>
           {renderNodeList(nodes.map(n => n.name))}
        </div>
      )}
    </div>
  );
};
