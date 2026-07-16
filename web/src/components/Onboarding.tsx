import { useEffect, useState } from 'react';
import { AlertCircle, CheckCircle2, Loader2, Play, Plus, RotateCw, Shield, X, Zap } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { api, ApiError, type SetupStatus } from '@/lib/api';
import { cn } from '@/lib/utils';
import { useToast } from './toast-context';

const dismissedKey = 'rweb-clash:onboarding-dismissed';

const SetupStep = ({
  done,
  title,
  desc,
}: {
  done: boolean;
  title: string;
  desc: string;
}) => (
  <div className={cn("flex min-w-0 max-w-full gap-3 rounded-2xl border p-3 sm:p-4", done ? "bg-green-500/5 border-green-500/20" : "bg-muted/30")}>
    <div className={cn("size-8 rounded-xl flex items-center justify-center shrink-0", done ? "bg-green-500 text-white" : "bg-muted text-muted-foreground")}>
      {done ? <CheckCircle2 className="size-4" /> : <AlertCircle className="size-4" />}
    </div>
    <div className="min-w-0 max-w-full text-left">
      <p className="text-sm font-black">{title}</p>
      <p className="mt-1 max-w-full text-xs font-bold text-muted-foreground [overflow-wrap:anywhere]">{desc}</p>
    </div>
  </div>
);

export const Onboarding = () => {
  const { toast } = useToast();
  const [status, setStatus] = useState<SetupStatus | null>(null);
  const [dismissed, setDismissed] = useState(() => localStorage.getItem(dismissedKey) === '1');
  const [url, setUrl] = useState('');
  const [name, setName] = useState('默认订阅');
  const [busy, setBusy] = useState<string | null>(null);

  const refresh = async () => {
    try {
      setStatus(await api.setupStatus());
    } catch {
      setStatus(null);
    }
  };

  useEffect(() => {
    let active = true;
    queueMicrotask(() => {
      void api.setupStatus().then(
        nextStatus => { if (active) setStatus(nextStatus); },
        () => { if (active) setStatus(null); },
      );
    });
    return () => { active = false; };
  }, []);

  if (dismissed || !status?.needsOnboarding) return null;

  const warnings = status.warnings;
  const canImport = url.trim().startsWith('http://') || url.trim().startsWith('https://');

  const dismiss = () => {
    localStorage.setItem(dismissedKey, '1');
    setDismissed(true);
  };

  const importSubscription = async () => {
    if (!canImport) {
      toast('订阅地址需要以 http:// 或 https:// 开头', 'error');
      return;
    }
    setBusy('import');
    try {
      await api.createSubscription({
        name: name.trim() || '默认订阅',
        url: url.trim(),
        interval: 360,
        inheritGlobal: true,
      });
      toast('订阅已导入', 'success');
      await refresh();
    } catch (error) {
      const message = error instanceof ApiError ? error.message : '订阅导入失败';
      toast(message, 'error');
    } finally {
      setBusy(null);
    }
  };

  const enableProxyAndStart = async () => {
    setBusy('start');
    try {
      await api.patchConfig({ system_proxy: true, auto_start: true });
      await api.startCore();
      toast('代理已启动', 'success');
      localStorage.setItem(dismissedKey, '1');
      setDismissed(true);
    } catch (error) {
      const message = error instanceof ApiError ? error.message : '启动失败，请查看运行日志';
      toast(message, 'error');
      await refresh();
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="fixed inset-0 z-[100] flex items-start justify-center overflow-x-hidden overflow-y-auto bg-background/70 p-2 backdrop-blur-md sm:items-center sm:p-4">
      <div className="my-auto min-w-0 flex-1 max-w-[calc(100vw-1rem)] overflow-hidden rounded-2xl border bg-card shadow-2xl sm:max-w-2xl sm:rounded-[2rem]">
        <div className="flex min-w-0 items-center justify-between gap-2 border-b bg-muted/20 p-4 sm:p-5">
          <div className="flex min-w-0 items-center gap-3">
            <div className="flex size-10 shrink-0 items-center justify-center rounded-2xl bg-primary text-primary-foreground">
              <Zap className="size-5" />
            </div>
            <div className="min-w-0 text-left">
              <h2 className="text-lg font-black">首次使用设置</h2>
              <p className="max-w-full text-[10px] font-bold uppercase tracking-widest text-muted-foreground [overflow-wrap:anywhere]">Import, start, connect</p>
            </div>
          </div>
          <Button variant="ghost" size="icon" onClick={dismiss} className="shrink-0 rounded-xl">
            <X className="size-5" />
          </Button>
        </div>

        <div className="flex min-w-0 flex-col gap-4 p-3 sm:gap-5 sm:p-5">
          <div className="grid min-w-0 gap-3 md:grid-cols-2">
            <SetupStep done={status.coreReady} title="内核资源" desc={status.coreReady ? 'Mihomo core 已就绪' : `未找到 core: ${status.corePath}`} />
            <SetupStep done={status.hasSubscriptions} title="订阅资源" desc={status.hasSubscriptions ? `已导入 ${status.subscriptionCount} 个订阅` : '先导入一个订阅地址'} />
          </div>

          {warnings.length > 0 && (
            <div className="flex min-w-0 max-w-full flex-col gap-2 rounded-2xl border border-amber-500/20 bg-amber-500/10 p-3 text-left sm:p-4">
              {warnings.map(item => (
                <p key={item} className="flex min-w-0 max-w-full gap-2 text-xs font-bold text-amber-700">
                  <AlertCircle className="size-4 shrink-0" />
                  <span className="min-w-0 max-w-full [overflow-wrap:anywhere]">{item}</span>
                </p>
              ))}
            </div>
          )}

          {!status.hasSubscriptions && (
            <div className="flex min-w-0 max-w-full flex-col gap-3 rounded-2xl border p-3 text-left sm:p-4">
              <div className="grid min-w-0 gap-3 md:grid-cols-[180px_1fr]">
                <input
                  value={name}
                  onChange={event => setName(event.target.value)}
                  className="min-w-0 max-w-full rounded-xl border bg-background px-3 py-3 text-sm font-bold outline-none ring-primary/10 focus:ring-4"
                  placeholder="订阅名称"
                />
                <input
                  value={url}
                  onChange={event => setUrl(event.target.value)}
                  className="min-w-0 max-w-full rounded-xl border bg-background px-3 py-3 text-sm font-mono outline-none ring-primary/10 focus:ring-4"
                  placeholder="https://example.com/sub"
                />
              </div>
              <Button onClick={importSubscription} disabled={busy !== null || !canImport} className="h-auto min-h-9 w-full min-w-0 max-w-full whitespace-normal rounded-xl px-3 py-2 text-center leading-tight font-black">
                {busy === 'import' ? <Loader2 className="size-4 animate-spin" /> : <Plus className="size-4" />}
                导入并同步订阅
              </Button>
            </div>
          )}

          <div className="grid min-w-0 gap-3 md:grid-cols-2">
            <Button variant="outline" onClick={refresh} disabled={busy !== null} className="h-auto min-h-12 min-w-0 max-w-full whitespace-normal rounded-xl px-3 py-3 text-center leading-tight font-black">
              <RotateCw className="size-4" />
              重新检测
            </Button>
            <Button onClick={enableProxyAndStart} disabled={busy !== null || !status.coreReady || !status.hasSubscriptions} className="h-auto min-h-12 min-w-0 max-w-full whitespace-normal rounded-xl px-3 py-3 text-center leading-tight font-black">
              {busy === 'start' ? <Loader2 className="size-4 animate-spin" /> : <Play className="size-4" />}
              <Shield className="size-4" />
              开启代理
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
};
