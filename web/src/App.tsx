import { useCallback, useEffect, useRef, useState, type FormEvent } from 'react';
import { Routes, Route, Navigate } from 'react-router-dom';
import { AlertCircle, Loader2, LockKeyhole, RotateCw } from 'lucide-react';
import { Layout } from './components/Layout';
import { Dashboard } from './components/Dashboard';
import { Proxies } from './components/Proxies';
import { Subscriptions } from './components/Subscriptions';
import { Rules } from './components/Rules';
import { Logs } from './components/Logs';
import { Settings } from './components/Settings';
import { Onboarding } from './components/Onboarding';
import { Button } from './components/ui/button';
import { api, ApiError, clearApiToken, setApiToken } from './lib/api';

type AccessState = 'checking' | 'locked' | 'error' | 'unlocked';

const isAuthenticationError = (error: unknown) =>
  error instanceof ApiError && (error.status === 401 || error.code === 'api_auth_required');

function App() {
  const [accessState, setAccessState] = useState<AccessState>('checking');
  const [token, setToken] = useState('');
  const [accessError, setAccessError] = useState<string | null>(null);
  const accessRequestId = useRef(0);

  const verifyAccess = useCallback(async (candidateToken?: string) => {
    const requestId = ++accessRequestId.current;
    if (candidateToken !== undefined) setApiToken(candidateToken);
    setAccessState('checking');
    setAccessError(null);

    try {
      await api.setupStatus();
      if (accessRequestId.current === requestId) setAccessState('unlocked');
    } catch (error) {
      if (accessRequestId.current !== requestId) return;
      if (isAuthenticationError(error)) {
        clearApiToken();
        setAccessError(candidateToken === undefined ? null : '令牌无效，请检查后重试。');
        setAccessState('locked');
      } else {
        setAccessError(error instanceof ApiError ? error.message : '无法连接到 RWeb Clash 服务。');
        setAccessState('error');
      }
    }
  }, []);

  useEffect(() => {
    queueMicrotask(() => void verifyAccess());
    return () => { accessRequestId.current += 1; };
  }, [verifyAccess]);

  const handleUnlock = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (token.trim()) void verifyAccess(token);
  };

  if (accessState === 'checking') {
    return (
      <div className="flex min-h-screen items-center justify-center bg-background text-foreground">
        <Loader2 className="size-8 animate-spin text-primary" />
      </div>
    );
  }

  if (accessState === 'locked') {
    return (
      <main className="flex min-h-screen items-center justify-center bg-background p-4 text-foreground">
        <form onSubmit={handleUnlock} className="w-full max-w-sm rounded-2xl border bg-card p-6 shadow-sm">
          <div className="flex items-center gap-3">
            <div className="flex size-10 items-center justify-center rounded-xl bg-primary text-primary-foreground">
              <LockKeyhole className="size-5" />
            </div>
            <div>
              <h1 className="text-lg font-black">API 访问验证</h1>
              <p className="text-xs font-bold text-muted-foreground">输入服务端配置的访问令牌</p>
            </div>
          </div>
          <label htmlFor="api-token" className="mt-6 block text-xs font-black text-muted-foreground">访问令牌</label>
          <input
            id="api-token"
            type="password"
            autoComplete="current-password"
            autoFocus
            value={token}
            onChange={event => setToken(event.target.value)}
            className="mt-2 h-11 w-full rounded-xl border bg-background px-4 font-mono text-sm outline-none focus:ring-2 focus:ring-ring"
          />
          {accessError && <p className="mt-3 flex items-center gap-2 text-xs font-bold text-destructive"><AlertCircle className="size-4 shrink-0" />{accessError}</p>}
          <Button type="submit" className="mt-5 w-full rounded-xl" disabled={!token.trim()}>
            解锁
          </Button>
        </form>
      </main>
    );
  }

  if (accessState === 'error') {
    return (
      <main className="flex min-h-screen items-center justify-center bg-background p-4 text-foreground">
        <div className="w-full max-w-sm rounded-2xl border bg-card p-6 text-center shadow-sm">
          <AlertCircle className="mx-auto size-8 text-destructive" />
          <h1 className="mt-4 text-lg font-black">服务暂时不可用</h1>
          <p className="mt-2 text-sm font-bold text-muted-foreground">{accessError}</p>
          <Button type="button" onClick={() => void verifyAccess()} className="mt-5 rounded-xl">
            <RotateCw />
            重试
          </Button>
        </div>
      </main>
    );
  }

  return (
    <Layout>
      <Onboarding />
      <Routes>
        <Route path="/" element={<Dashboard />} />
        <Route path="/subscriptions" element={<Subscriptions />} />
        <Route path="/proxies" element={<Proxies />} />
        <Route path="/rules" element={<Rules />} />
        <Route path="/logs" element={<Logs />} />
        <Route path="/settings" element={<Settings />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </Layout>
  );
}

export default App;
