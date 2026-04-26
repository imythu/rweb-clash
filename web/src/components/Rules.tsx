import { useState, useEffect } from 'react';
import { 
  Search, 
  Plus, 
  Shield, 
  Zap, 
  Trash2, 
  ChevronRight, 
  Loader2, 
  CheckCircle2, 
  FlaskConical, 
  Target,
  Layers,
  Settings2
} from 'lucide-react';
import { Button } from "@/components/ui/button";
import { useToast } from './Toast';

export const Rules = () => {
  const { toast } = useToast();
  const [rules, setRules] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const [testTarget, setTarget] = useState('');
  const [testResult, setTestResult] = useState<any>(null);
  const [isTesting, setIsTesting] = useState(false);

  useEffect(() => { fetchRules(); }, []);

  const fetchRules = async () => {
    const res = await fetch('/api/rules');
    const data = await res.json();
    setRules(data);
    setLoading(false);
  };

  const handleTestSandbox = async () => {
    if (!testTarget) return;
    setIsTesting(true);
    try {
      const res = await fetch('/api/rules/test', {
        method: 'POST',
        body: JSON.stringify({ target: testTarget })
      });
      const data = await res.json();
      setTestResult(data);
      toast('规则沙盒分析完成', 'success');
    } finally {
      setIsTesting(false);
    }
  };

  if (loading) return <div className="flex items-center justify-center h-[60vh]"><Loader2 className="animate-spin text-primary size-8" /></div>;

  return (
    <div className="space-y-10 max-w-7xl mx-auto pb-20 animate-in fade-in duration-500 text-left">
      {/* Header */}
      <div className="flex justify-between items-end text-left px-2">
        <div>
          <h2 className="text-4xl font-black uppercase tracking-tighter">路由决策中心</h2>
          <p className="text-sm text-muted-foreground font-bold mt-2 flex items-center gap-2 tracking-tight">
            <Target className="size-4 text-primary" /> 可视化分流逻辑 · 精准控制流量流向
          </p>
        </div>
        <div className="flex gap-4">
           <Button variant="outline" className="rounded-2xl gap-2 font-black text-xs h-12 px-6 uppercase border-2 shadow-sm">
             导入规则集
           </Button>
           <Button onClick={() => toast('引导式规则编辑器正在开发中...', 'info')} className="rounded-2xl gap-2 shadow-xl shadow-primary/30 font-black text-xs h-12 px-8 uppercase hover:scale-105 transition-all">
             <Plus className="size-5" /> 新增规则积木
           </Button>
        </div>
      </div>

      {/* RULE SANDBOX - DECISION VISUALIZATION */}
      <div className="grid grid-cols-1 lg:grid-cols-12 gap-8 px-2">
         <div className="lg:col-span-4 space-y-6">
            <div className="bg-zinc-900 text-white rounded-[2.5rem] p-8 shadow-2xl relative overflow-hidden group">
               <div className="relative z-10 space-y-6 text-left">
                  <div className="flex items-center gap-4 text-left">
                     <div className="size-12 rounded-2xl bg-blue-500 text-white flex items-center justify-center shadow-lg shadow-blue-500/20">
                        <FlaskConical className="size-6" />
                     </div>
                     <div className="text-left">
                        <h3 className="text-xl font-black uppercase tracking-tight">规则沙盒</h3>
                        <p className="text-[10px] font-black text-blue-400 uppercase tracking-widest">Logic Laboratory</p>
                     </div>
                  </div>
                  <p className="text-xs text-zinc-400 font-bold leading-relaxed text-left">
                    输入域名或 IP，模拟分流过程，实时预览请求命中哪条规则。
                  </p>
                  <div className="space-y-4">
                     <div className="relative">
                        <Search className="absolute left-4 top-1/2 -translate-y-1/2 size-4 text-zinc-500" />
                        <input 
                           value={testTarget}
                           onChange={(e) => setTarget(e.target.value)}
                           onKeyDown={(e) => e.key === 'Enter' && handleTestSandbox()}
                           placeholder="测试地址: google.com" 
                           className="w-full pl-12 pr-4 py-4 bg-white/5 border border-white/10 rounded-2xl text-sm font-black outline-none focus:border-blue-500 transition-all placeholder:text-zinc-600 shadow-inner" 
                        />
                     </div>
                     <Button 
                        disabled={isTesting || !testTarget}
                        onClick={handleTestSandbox}
                        className="w-full h-14 bg-blue-600 hover:bg-blue-500 text-white rounded-2xl font-black uppercase tracking-widest shadow-xl shadow-blue-600/20 active:scale-95 transition-all"
                     >
                        {isTesting ? <Loader2 className="animate-spin size-5" /> : '开始模拟分析'}
                     </Button>
                  </div>
               </div>
               <Shield className="absolute -right-16 -bottom-16 size-64 text-white/[0.02] -rotate-12 pointer-events-none" />
            </div>

            {testResult && (
               <div className="bg-card border-2 border-primary/20 rounded-[2.5rem] p-8 space-y-6 animate-in slide-in-from-top-4 duration-500 text-left shadow-lg">
                  <h4 className="text-[10px] font-black uppercase tracking-[0.2em] text-primary flex items-center gap-2">
                     <CheckCircle2 className="size-3" /> 分析结果反馈
                  </h4>
                  <div className="space-y-4 text-left">
                     <div className="flex items-center gap-4 text-left">
                        <div className="size-10 rounded-xl bg-muted flex items-center justify-center font-black text-[10px] uppercase text-muted-foreground border">HIT</div>
                        <div className="text-left">
                           <p className="text-[10px] font-black text-muted-foreground uppercase mb-0.5 tracking-tighter">命中规则类型</p>
                           <p className="text-sm font-black font-mono">{testResult.hitRule.type}</p>
                        </div>
                     </div>
                     <div className="flex items-center gap-4 text-left">
                        <div className="size-10 rounded-xl bg-primary/10 text-primary flex items-center justify-center"><ChevronRight className="size-5" /></div>
                        <div className="text-left text-left">
                           <p className="text-[10px] font-black text-primary uppercase mb-0.5 tracking-tighter">最终出口节点</p>
                           <p className="text-sm font-black text-primary">{testResult.finalProxy}</p>
                        </div>
                     </div>
                  </div>
                  <div className="p-4 bg-muted/30 rounded-2xl border border-dashed border-muted-foreground/20">
                     <p className="text-[10px] font-bold text-muted-foreground leading-relaxed text-left">
                        该请求由于匹配了 <span className="text-foreground">"{testResult.hitRule.value}"</span> 规则，被转发至 <span className="text-primary">{testResult.hitRule.policy}</span> 策略组。
                     </p>
                  </div>
               </div>
            )}
         </div>

         <div className="lg:col-span-8 space-y-6 text-left">
            <div className="flex items-center gap-4 opacity-60 ml-2 text-left">
               <Layers className="size-4" />
               <h3 className="text-[10px] font-black uppercase tracking-[0.3em]">活跃路由清单 (Rules Active)</h3>
               <div className="h-px flex-1 bg-muted" />
            </div>
            
            <div className="bg-card border-2 rounded-[2.5rem] overflow-hidden shadow-sm">
               <div className="p-6 border-b bg-muted/10 flex justify-between items-center text-left">
                  <div className="flex items-center gap-4 text-left text-left">
                     <div className="relative">
                        <Search className="absolute left-4 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
                        <input placeholder="快速过滤规则..." className="pl-11 pr-4 py-2.5 bg-background border-2 rounded-xl text-[11px] font-black uppercase outline-none focus:border-primary/50 w-64 shadow-sm" />
                     </div>
                  </div>
                  <div className="flex gap-2">
                     <div className="px-3 py-1 bg-primary/5 border rounded-lg text-[9px] font-black text-primary uppercase tracking-tighter flex items-center gap-1.5"><Shield className="size-2.5" /> DOMAIN: 1240</div>
                     <div className="px-3 py-1 bg-muted border rounded-lg text-[9px] font-black text-muted-foreground uppercase tracking-tighter">IP: 452</div>
                  </div>
               </div>

               <div className="divide-y-2 border-muted/50 text-left">
                  {rules.map((rule) => (
                    <div key={rule.id} className="p-6 flex items-center justify-between hover:bg-muted/30 transition-all group text-left">
                       <div className="flex items-center gap-8 flex-1 text-left text-left">
                          <div className="w-40 shrink-0 text-left">
                             <div className="px-3 py-1.5 bg-muted/50 rounded-xl border flex items-center justify-center gap-2 group-hover:border-primary/30 transition-colors">
                                <span className="text-[9px] font-black text-muted-foreground uppercase tracking-widest">{rule.type}</span>
                             </div>
                          </div>
                          <div className="flex-1 text-left text-left text-left">
                             <p className="font-mono text-sm font-black text-left">{rule.value}</p>
                             <p className="text-[9px] font-bold text-muted-foreground uppercase opacity-50 tracking-wider mt-0.5 text-left">{rule.desc}</p>
                          </div>
                       </div>
                       
                       <div className="flex items-center gap-8 shrink-0 text-left">
                          <div className="flex items-center gap-3 bg-primary/[0.03] px-5 py-3 rounded-2xl border-2 border-primary/10 shadow-sm min-w-[140px] text-left">
                             <div className="size-2 rounded-full bg-primary animate-pulse" />
                             <span className="text-xs font-black uppercase tracking-tight text-primary">{rule.policy}</span>
                          </div>
                          <div className="flex gap-1 opacity-0 group-hover:opacity-100 transition-all">
                             <Button variant="ghost" size="icon" className="size-10 rounded-xl hover:bg-muted"><Settings2 className="size-4" /></Button>
                             <Button variant="ghost" size="icon" className="size-10 rounded-xl hover:bg-red-500/10 text-destructive"><Trash2 className="size-4" /></Button>
                          </div>
                       </div>
                    </div>
                  ))}
               </div>
            </div>
            
            <div className="p-8 border-2 rounded-[2rem] border-dashed opacity-40 grayscale hover:opacity-100 hover:grayscale-0 transition-all flex items-center justify-between text-left">
               <div className="flex items-center gap-4 text-left">
                  <Zap className="size-5 text-primary" />
                  <p className="text-[10px] font-bold text-muted-foreground uppercase tracking-widest">
                     分流提示: 规则由上至下匹配，一旦命中即刻执行动作。建议将 <span className="text-primary underline">DOMAIN</span> 类规则置顶。
                  </p>
               </div>
            </div>
         </div>
      </div>
    </div>
  );
};
