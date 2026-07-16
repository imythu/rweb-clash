import { useState, useEffect, useRef, useCallback } from 'react';
import { 
  Search, 
  Trash2, 
  Download, 
  Terminal,
  PlusCircle,
  ShieldCheck,
  Globe,
  X,
  ChevronRight,
  RefreshCcw
} from 'lucide-react';
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { useToast } from './toast-context';
import { api, type LogEntry } from '@/lib/api';
import { usePageActivity } from '@/lib/usePageActivity';

const LOG_POLL_MS = 5000;
const AUTO_FOLLOW_THRESHOLD_PX = 80;

interface ConnectionInfo {
  domain: string;
  ruleType: string;
  ruleValue: string;
  currentPolicy: string;
}

export const Logs = () => {
  const { toast } = useToast();
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [filter, setFilter] = useState('all');
  const [search, setSearch] = useState('');
  const [isLoading, setIsLoading] = useState(true);
  const [proxyGroups, setProxyGroups] = useState<string[]>([]);
  const scrollRef = useRef<HTMLDivElement>(null);
  const logsInFlight = useRef(false);
  const mountedRef = useRef(false);
  const shouldAutoFollow = useRef(true);
  const isPageActive = usePageActivity();
  
  const [selectedConn, setSelectedConn] = useState<ConnectionInfo | null>(null);
  const [newRulePolicy, setNewRulePolicy] = useState('');
  const [newRuleType, setNewRuleType] = useState('DOMAIN-SUFFIX');

  const fetchLogs = useCallback(async () => {
    if (!isPageActive || document.hidden || logsInFlight.current) return;
    logsInFlight.current = true;
    try {
      const nextLogs = await api.listLogs();
      if (mountedRef.current) setLogs(nextLogs);
    } catch (error) {
      console.error('Failed to fetch logs:', error);
    } finally {
      logsInFlight.current = false;
      if (mountedRef.current) setIsLoading(false);
    }
  }, [isPageActive]);

  const fetchProxyGroups = useCallback(async () => {
    try {
      const data = await api.proxyTopology();
      const groupNames = data.groups.map(group => group.name);
      if (mountedRef.current) setProxyGroups(['DIRECT', 'REJECT', ...groupNames]);
    } catch (error) {
      console.error('Failed to fetch proxy groups:', error);
    }
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    queueMicrotask(() => void fetchProxyGroups());
    return () => {
      mountedRef.current = false;
    };
  }, [fetchProxyGroups]);

  useEffect(() => {
    if (!isPageActive) return;

    const refresh = () => void fetchLogs();
    const handleVisibilityChange = () => {
      if (!document.hidden) refresh();
    };

    queueMicrotask(refresh);
    document.addEventListener('visibilitychange', handleVisibilityChange);
    const interval = window.setInterval(refresh, LOG_POLL_MS);
    return () => {
      document.removeEventListener('visibilitychange', handleVisibilityChange);
      window.clearInterval(interval);
    };
  }, [fetchLogs, isPageActive]);

  useEffect(() => {
    if (!shouldAutoFollow.current || !scrollRef.current) return;
    const frame = window.requestAnimationFrame(() => {
      if (mountedRef.current && shouldAutoFollow.current && scrollRef.current) {
        scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
      }
    });
    return () => window.cancelAnimationFrame(frame);
  }, [logs]);

  const handleLogScroll = () => {
    const element = scrollRef.current;
    if (!element) return;
    const distanceFromBottom = element.scrollHeight - element.scrollTop - element.clientHeight;
    shouldAutoFollow.current = distanceFromBottom <= AUTO_FOLLOW_THRESHOLD_PX;
  };

  const parseConnection = (payload: string): ConnectionInfo | null => {
    const regex = /\[(?:TCP|UDP)\]\s+[\d.]+:\d+\s+-->\s+([^:]+):\d+\s+match\s+([^(]+)\(([^)]+)\)\s+using\s+([^[\n]+)/;
    const match = payload.match(regex);
    if (match) {
      return {
        domain: match[1],
        ruleType: match[2],
        ruleValue: match[3],
        currentPolicy: match[4].trim()
      };
    }
    return null;
  };

  const handleCreateRule = async () => {
    if (!selectedConn || !newRulePolicy) return;
    try {
      await api.createRule({
        type: newRuleType,
        value: newRuleType === 'DOMAIN-KEYWORD' ? selectedConn.domain.split('.')[0] : selectedConn.domain,
        policy: newRulePolicy,
        desc: `From Log: ${selectedConn.domain}`
      });
      toast('自定义规则已添加', 'success');
      setSelectedConn(null);
    } catch {
      toast('网络异常', 'error');
    }
  };

  const handleClearLogs = async () => {
    try {
      await api.clearLogs();
      shouldAutoFollow.current = true;
      setLogs([]);
      toast('日志已清空', 'success');
    } catch {
      toast('清空失败', 'error');
    }
  };

  const handleExportLogs = async () => {
    try {
      const text = await api.exportLogs();
      const url = URL.createObjectURL(new Blob([text], { type: 'text/plain;charset=utf-8' }));
      const link = document.createElement('a');
      link.href = url;
      link.download = 'rweb-clash.log';
      link.click();
      URL.revokeObjectURL(url);
    } catch {
      toast('导出失败', 'error');
    }
  };

  const handleExportDiagnostics = async () => {
    try {
      const text = await api.exportDiagnostics();
      const url = URL.createObjectURL(new Blob([text], { type: 'text/markdown;charset=utf-8' }));
      const link = document.createElement('a');
      link.href = url;
      link.download = 'rweb-clash-diagnostics.md';
      link.click();
      URL.revokeObjectURL(url);
    } catch {
      toast('诊断包导出失败', 'error');
    }
  };

  const filteredLogs = logs.filter(log => {
    const matchesFilter = filter === 'all' || log.level === filter;
    const matchesSearch = log.payload.toLowerCase().includes(search.toLowerCase());
    return matchesFilter && matchesSearch;
  });

  return (
    <div className="space-y-6 md:space-y-10 max-w-7xl mx-auto pb-12 animate-in fade-in duration-500 text-left">
      
      {/* Header Area */}
      <div className="flex flex-col md:flex-row md:items-end justify-between gap-6 px-1">
        <div className="space-y-2 text-left">
          <div className="flex items-center gap-3">
            <div className="size-12 bg-primary/10 rounded-[1.25rem] flex items-center justify-center text-primary shadow-inner">
              <Terminal className="size-6" />
            </div>
            <h1 className="text-3xl md:text-4xl font-black uppercase tracking-tighter">运行日志</h1>
          </div>
          <p className="text-xs md:text-sm font-bold text-muted-foreground uppercase tracking-wider ml-1">Live Engine Diagnostic Stream</p>
        </div>
        
        <div className="flex flex-wrap items-center gap-3">
          <div className="relative group min-w-[200px]">
            <Search className="absolute left-4 top-1/2 -translate-y-1/2 size-4 text-muted-foreground transition-colors group-focus-within:text-primary" />
            <input 
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="搜索流量详情..." 
              className="h-14 pl-12 pr-4 bg-card border rounded-2xl w-full text-sm font-bold outline-none focus:border-primary/30 transition-all shadow-sm"
            />
          </div>
          <Button onClick={handleClearLogs} variant="outline" className="h-14 px-6 rounded-2xl font-black uppercase tracking-widest border bg-muted hover:bg-red-500/10 hover:text-red-500 transition-all">
            <Trash2 className="size-5" />
          </Button>
          <Button onClick={handleExportDiagnostics} variant="outline" className="h-14 px-6 rounded-2xl font-black uppercase tracking-widest border bg-muted hover:bg-primary/10 hover:text-primary transition-all">
            <Download className="size-5" />
            诊断包
          </Button>
        </div>
      </div>

      {/* Filter Segmented Control */}
      <div className="px-1">
        <div className="bg-muted p-1.5 rounded-2xl flex items-center gap-1.5 border max-w-md">
          {['all', 'info', 'warning', 'error'].map((l) => (
            <button
              key={l}
              onClick={() => setFilter(l)}
              className={cn(
                "flex-1 py-2.5 rounded-xl text-[10px] md:text-xs font-black uppercase tracking-widest transition-all duration-300",
                filter === l 
                  ? "bg-card text-primary shadow-sm" 
                  : "text-muted-foreground hover:text-foreground"
              )}
            >
              {l}
            </button>
          ))}
        </div>
      </div>

      {/* Glass Console Container */}
      <div className="bg-card border rounded-[2.5rem] shadow-md overflow-hidden flex flex-col h-[65vh] relative group text-left">
        
        {/* Console Top Bar */}
        <div className="px-8 py-4 border-b bg-muted/50 flex items-center justify-between shrink-0">
          <div className="flex items-center gap-4">
            <div className="flex gap-2">
              <div className="size-3 rounded-full bg-red-500/20 border border-red-500/40" />
              <div className="size-3 rounded-full bg-amber-500/20 border border-amber-500/40" />
              <div className="size-3 rounded-full bg-green-500/20 border border-green-500/40" />
            </div>
            <div className="h-4 w-px bg-border mx-2" />
            <span className="text-[10px] font-black uppercase tracking-widest text-muted-foreground/60">mihomo_core_diagnostic_stream.log</span>
          </div>
          <button onClick={handleExportLogs} className="flex items-center gap-2 text-[10px] font-black uppercase tracking-widest text-primary hover:opacity-70 transition-opacity">
            <Download className="size-3" /> 导出全部记录
          </button>
        </div>

        {/* Log Content Area */}
        <div ref={scrollRef} onScroll={handleLogScroll} className="flex-1 overflow-y-auto p-6 md:p-10 font-mono space-y-2 custom-scrollbar scroll-smooth">
          {isLoading ? (
            <div className="flex flex-col items-center justify-center h-full space-y-6">
              <RefreshCcw className="size-10 text-primary animate-spin opacity-20" />
              <p className="text-xs font-black uppercase tracking-wider text-muted-foreground animate-pulse">Initializing Socket...</p>
            </div>
          ) : filteredLogs.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-full space-y-4 opacity-60">
              <Globe className="size-24" />
              <p className="text-2xl font-black uppercase tracking-[0.5em]">No Logs</p>
            </div>
          ) : (
            <div className="space-y-1.5 text-left">
              {filteredLogs.map((log, idx) => {
                const conn = parseConnection(log.payload);
                return (
                  <div 
                    key={idx} 
                    onClick={() => conn && setSelectedConn(conn)}
                    className={cn(
                      "flex items-start gap-4 p-2 rounded-xl transition-all group/line text-left",
                      conn ? "cursor-pointer hover:bg-primary/[0.03]" : "hover:bg-muted/30"
                    )}
                  >
                    <span className="text-[10px] font-bold text-muted-foreground shrink-0 w-20 leading-6">{log.time.split(' ')[1]}</span>
                    <span className={cn(
                      "px-2 py-0.5 rounded text-[9px] font-black uppercase tracking-tighter shrink-0 mt-1",
                      log.level === 'info' ? "bg-blue-500/10 text-blue-500" :
                      log.level === 'warning' ? "bg-amber-500/10 text-amber-500" :
                      log.level === 'error' ? "bg-red-500/10 text-red-500" : "bg-muted text-muted-foreground"
                    )}>
                      {log.level}
                    </span>
                    <div className="flex-1 min-w-0 flex items-center gap-4">
                      <p className={cn(
                        "text-xs md:text-sm font-bold tracking-tight leading-6 break-all text-left",
                        conn ? "text-foreground group-hover/line:text-primary transition-colors" : "text-muted-foreground"
                      )}>
                        {log.payload}
                      </p>
                      {conn && (
                        <div className="opacity-0 group-hover/line:opacity-100 transition-all bg-zinc-900 text-white text-[10px] font-black px-2 py-1 rounded-lg uppercase flex items-center gap-1.5 shrink-0 shadow-lg">
                           <PlusCircle className="size-3" /> 捷径
                        </div>
                      )}
                    </div>
                  </div>
                );
              })}
              <div className="flex items-center gap-4 p-2 text-left">
                 <div className="size-1.5 bg-green-500 rounded-full animate-pulse shadow-[0_0_8px_rgba(34,197,94,0.5)]" />
                 <span className="text-[10px] font-black uppercase tracking-widest text-muted-foreground animate-pulse">Waiting for network events...</span>
              </div>
            </div>
          )}
        </div>
      </div>

      {/* Modal - Consistent with Dashboard Logic Forge */}
      {selectedConn && (
        <div className="fixed inset-0 z-[100] flex items-center justify-center p-4 overflow-hidden">
          <div className="absolute inset-0 bg-background/60 backdrop-blur-md animate-in fade-in duration-500" onClick={() => setSelectedConn(null)} />
          <div className="relative w-full max-w-lg bg-card border-2 border-primary/10 rounded-[2.5rem] shadow-2xl animate-in zoom-in-95 duration-500 flex flex-col overflow-hidden text-left">
             <div className="p-8 md:p-10 space-y-10">
                <div className="flex justify-between items-start">
                  <div className="text-left">
                    <h3 className="text-2xl font-black uppercase tracking-tighter flex items-center gap-3">
                      <ShieldCheck className="size-7 text-primary" />
                      规则快捷实验室
                    </h3>
                    <p className="text-[10px] font-bold text-muted-foreground uppercase tracking-widest mt-1 opacity-60">Instant Traffic Redirection</p>
                  </div>
                  <Button variant="ghost" size="icon" onClick={() => setSelectedConn(null)} className="size-10 rounded-xl"><X className="size-5" /></Button>
                </div>

                <div className="space-y-6">
                   <div className="p-6 rounded-[2rem] bg-muted/30 border border-muted/50 space-y-4 text-left">
                      <div className="flex items-center gap-3 opacity-70 text-[10px] font-black uppercase tracking-widest">
                        <Globe className="size-3" /> 目标资产画像
                      </div>
                      <div className="text-2xl font-black tracking-tighter truncate text-primary">{selectedConn.domain}</div>
                      <div className="flex items-center gap-3 text-[10px] font-bold text-muted-foreground uppercase">
                        <span className="bg-card px-2 py-1 rounded-md border text-foreground/60">{selectedConn.ruleType}</span>
                        <ChevronRight className="size-3" />
                        <span className="text-primary font-black">{selectedConn.currentPolicy}</span>
                      </div>
                   </div>

                   <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                      <div className="space-y-2 text-left">
                        <label className="text-[10px] font-black uppercase tracking-widest text-muted-foreground ml-3">匹配范围</label>
                        <select value={newRuleType} onChange={e => setNewRuleType(e.target.value)} className="w-full h-14 bg-muted/50 border-2 border-transparent focus:border-primary/20 rounded-2xl px-5 text-sm font-black outline-none appearance-none transition-all">
                           <option value="DOMAIN-SUFFIX">域名后缀</option>
                           <option value="DOMAIN">精确域名</option>
                           <option value="DOMAIN-KEYWORD">关键词</option>
                        </select>
                      </div>
                      <div className="space-y-2 text-left">
                        <label className="text-[10px] font-black uppercase tracking-widest text-muted-foreground ml-3">部署出口</label>
                        <select value={newRulePolicy} onChange={e => setNewRulePolicy(e.target.value)} className="w-full h-14 bg-muted/50 border-2 border-transparent focus:border-primary/20 rounded-2xl px-5 text-sm font-black outline-none appearance-none transition-all">
                           <option value="">选择策略...</option>
                           {proxyGroups.map(g => <option key={g} value={g}>{g}</option>)}
                        </select>
                      </div>
                   </div>
                </div>

                <div className="flex gap-4">
                   <Button variant="ghost" onClick={() => setSelectedConn(null)} className="flex-1 h-16 rounded-[1.5rem] font-black uppercase tracking-widest text-muted-foreground hover:bg-muted">取消</Button>
                   <Button onClick={handleCreateRule} disabled={!newRulePolicy} className="flex-[2] h-16 bg-zinc-900 text-white hover:bg-black rounded-[1.5rem] font-black uppercase tracking-wider shadow-xl transition-all active:scale-95">
                     部署出口规则
                   </Button>
                </div>
             </div>
             <div className="bg-primary/5 p-4 text-center border-t border-primary/10">
                <p className="text-[9px] font-black uppercase text-primary/60 tracking-[0.1em]">
                  此操作将立即在“路由管理”中生成一条权重最高的自定义规则
                </p>
             </div>
          </div>
        </div>
      )}
    </div>
  );
};
