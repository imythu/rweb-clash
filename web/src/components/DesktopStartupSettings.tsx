import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { CircleAlert, RotateCw } from 'lucide-react';

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldTitle,
} from '@/components/ui/field';
import { Spinner } from '@/components/ui/spinner';
import { Switch } from '@/components/ui/switch';
import { useToast } from './toast-context';

type DesktopPreferences = {
  launch_at_login: boolean;
  silent_start: boolean;
};

type PreferenceKey = keyof DesktopPreferences;

export function DesktopStartupSettings() {
  const { toast } = useToast();
  const [preferences, setPreferences] = useState<DesktopPreferences | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [pending, setPending] = useState<PreferenceKey | null>(null);
  const requestId = useRef(0);
  const updateInFlight = useRef(false);
  const desktop = isTauri();

  const load = useCallback(async () => {
    if (!desktop) return;
    const currentRequest = ++requestId.current;
    setLoadError(null);
    try {
      const next = await invoke<DesktopPreferences>('get_desktop_preferences');
      if (requestId.current === currentRequest) setPreferences(next);
    } catch {
      if (requestId.current === currentRequest) {
        setLoadError('桌面启动设置加载失败。');
      }
    }
  }, [desktop]);

  useEffect(() => {
    queueMicrotask(() => void load());
    return () => {
      requestId.current += 1;
    };
  }, [load]);

  const update = async (key: PreferenceKey, enabled: boolean) => {
    if (!preferences || updateInFlight.current) return;
    const previous = preferences;
    updateInFlight.current = true;
    setPending(key);
    setPreferences({ ...preferences, [key]: enabled });
    try {
      const command = key === 'launch_at_login' ? 'set_launch_at_login' : 'set_silent_start';
      const next = await invoke<DesktopPreferences>(command, { enabled });
      setPreferences(next);
      toast('桌面启动设置已更新', 'success');
    } catch {
      setPreferences(previous);
      toast('桌面启动设置更新失败', 'error');
    } finally {
      updateInFlight.current = false;
      setPending(null);
    }
  };

  if (!desktop) return null;

  if (loadError) {
    return (
      <Alert variant="destructive">
        <CircleAlert />
        <AlertTitle>无法读取桌面设置</AlertTitle>
        <AlertDescription className="flex items-center justify-between gap-4">
          <span>{loadError}</span>
          <Button type="button" variant="outline" size="sm" onClick={() => void load()}>
            <RotateCw data-icon="inline-start" />
            重试
          </Button>
        </AlertDescription>
      </Alert>
    );
  }

  if (!preferences) {
    return (
      <div className="flex min-h-28 items-center justify-center gap-2 text-muted-foreground">
        <Spinner />
        <span className="text-sm">正在读取桌面设置</span>
      </div>
    );
  }

  return (
    <FieldGroup className="gap-5" aria-busy={pending !== null}>
      <Field orientation="horizontal" data-disabled={pending !== null || undefined}>
        <FieldContent>
          <FieldTitle>登录时启动桌面端</FieldTitle>
          <FieldDescription>进入桌面系统后自动运行 R-Clash。</FieldDescription>
        </FieldContent>
        <Switch
          aria-label="登录时启动桌面端"
          checked={preferences.launch_at_login}
          disabled={pending !== null}
          onCheckedChange={enabled => void update('launch_at_login', enabled)}
        />
      </Field>
      <Field orientation="horizontal" data-disabled={pending !== null || undefined}>
        <FieldContent>
          <FieldTitle>静默启动</FieldTitle>
          <FieldDescription>登录启动时只显示托盘，不打开主窗口。</FieldDescription>
        </FieldContent>
        <Switch
          aria-label="静默启动"
          checked={preferences.silent_start}
          disabled={pending !== null}
          onCheckedChange={enabled => void update('silent_start', enabled)}
        />
      </Field>
    </FieldGroup>
  );
}
