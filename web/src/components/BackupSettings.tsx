import { useCallback, useEffect, useRef, useState } from 'react';
import { Cloud, DatabaseBackup, RefreshCw, RotateCcw, Trash2 } from 'lucide-react';
import {
  api,
  type Backup,
  type WebDavSettings,
  type WebDavSettingsInput,
} from '@/lib/api';
import { useToast } from './toast-context';
import { Button } from '@/components/ui/button';
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldTitle,
} from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { Spinner } from '@/components/ui/spinner';
import { Separator } from '@/components/ui/separator';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty';

type Draft = {
  endpoint: string;
  username: string;
  password: string;
  remotePath: string;
  enabled: boolean;
  autoSync: boolean;
  intervalHours: number;
  retention: number;
};

const EMPTY_DRAFT: Draft = {
  endpoint: '',
  username: '',
  password: '',
  remotePath: 'rweb-clash',
  enabled: false,
  autoSync: false,
  intervalHours: 24,
  retention: 7,
};

function settingsDraft(settings: WebDavSettings): Draft {
  return {
    endpoint: settings.endpoint,
    username: settings.username,
    password: '',
    remotePath: settings.remotePath,
    enabled: settings.enabled,
    autoSync: settings.autoSync,
    intervalHours: settings.intervalHours,
    retention: settings.retention,
  };
}

function formatSize(bytes: number) {
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${bytes} B`;
}

export function BackupSettings() {
  const { toast } = useToast();
  const [settings, setSettings] = useState<WebDavSettings | null>(null);
  const [draft, setDraft] = useState<Draft>(EMPTY_DRAFT);
  const [backups, setBackups] = useState<Backup[]>([]);
  const [loading, setLoading] = useState(true);
  const [operation, setOperation] = useState<string | null>(null);
  const [restoreTarget, setRestoreTarget] = useState<Backup | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<Backup | null>(null);
  const [confirmRemoteRestore, setConfirmRemoteRestore] = useState(false);
  const operationInFlight = useRef(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [nextSettings, nextBackups] = await Promise.all([
        api.webdavSettings(),
        api.listBackups(),
      ]);
      setSettings(nextSettings);
      setDraft(settingsDraft(nextSettings));
      setBackups(nextBackups);
    } catch {
      toast('备份配置加载失败', 'error');
    } finally {
      setLoading(false);
    }
  }, [toast]);

  useEffect(() => {
    queueMicrotask(() => void load());
  }, [load]);

  const withOperation = async (name: string, action: () => Promise<void>) => {
    if (operationInFlight.current) return;
    operationInFlight.current = true;
    setOperation(name);
    try {
      await action();
    } finally {
      operationInFlight.current = false;
      setOperation(null);
    }
  };

  const settingsInput = (): WebDavSettingsInput => ({
    endpoint: draft.endpoint.trim(),
    username: draft.username.trim(),
    password: draft.password || undefined,
    remotePath: draft.remotePath.trim(),
    enabled: draft.enabled,
    autoSync: draft.autoSync,
    intervalHours: Math.max(1, draft.intervalHours),
    retention: Math.max(1, draft.retention),
  });

  const saveSettings = async () => {
    const saved = await api.saveWebdavSettings(settingsInput());
    setSettings(saved);
    setDraft(settingsDraft(saved));
    return saved;
  };

  const save = () => void withOperation('save', async () => {
    try {
      await saveSettings();
      toast('WebDAV 配置已保存', 'success');
    } catch {
      toast('WebDAV 配置保存失败', 'error');
    }
  });

  const test = () => void withOperation('test', async () => {
    try {
      await saveSettings();
      await api.testWebdav();
      toast('WebDAV 连接成功', 'success');
    } catch {
      toast('WebDAV 连接失败', 'error');
    }
  });

  const createLocal = () => void withOperation('create', async () => {
    try {
      const backup = await api.createBackup();
      setBackups(current => [backup, ...current.filter(item => item.name !== backup.name)]);
      toast('本地备份已创建', 'success');
    } catch {
      toast('本地备份创建失败', 'error');
    }
  });

  const sync = () => void withOperation('sync', async () => {
    try {
      await saveSettings();
      const backup = await api.syncWebdav();
      setBackups(current => [backup, ...current.filter(item => item.name !== backup.name)]);
      setSettings(await api.webdavSettings());
      toast('备份已同步到 WebDAV', 'success');
    } catch {
      toast('WebDAV 同步失败', 'error');
    }
  });

  const remove = (backup: Backup) => void withOperation(`delete:${backup.name}`, async () => {
    try {
      await api.deleteBackup(backup.name);
      setBackups(current => current.filter(item => item.name !== backup.name));
      setDeleteTarget(null);
      toast('备份已删除', 'success');
    } catch {
      toast('备份删除失败', 'error');
    }
  });

  const restoreLocal = () => void withOperation('restore-local', async () => {
    if (!restoreTarget) return;
    try {
      await api.restoreBackup(restoreTarget.name);
      toast('备份已恢复，内核保持停止', 'success');
      setRestoreTarget(null);
      window.location.reload();
    } catch {
      toast('备份恢复失败', 'error');
    }
  });

  const restoreRemote = () => void withOperation('restore-remote', async () => {
    try {
      await saveSettings();
      await api.restoreWebdav();
      toast('WebDAV 备份已恢复，内核保持停止', 'success');
      setConfirmRemoteRestore(false);
      window.location.reload();
    } catch {
      toast('WebDAV 恢复失败', 'error');
    }
  });

  if (loading) {
    return <div className="flex min-h-48 items-center justify-center"><Spinner /></div>;
  }

  return (
    <section className="border-y py-8">
      <div className="mb-6 flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
        <div>
          <h2 className="text-xl font-semibold">同步与备份</h2>
          <p className="mt-1 text-sm text-muted-foreground">
            {settings?.lastSync ? `上次同步 ${settings.lastSync}` : '尚未执行同步'}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button type="button" variant="outline" disabled={operation !== null} onClick={test}>
            {operation === 'test' ? <Spinner data-icon="inline-start" /> : <Cloud data-icon="inline-start" />}
            测试连接
          </Button>
          <Button type="button" variant="outline" disabled={operation !== null} onClick={createLocal}>
            {operation === 'create' ? <Spinner data-icon="inline-start" /> : <DatabaseBackup data-icon="inline-start" />}
            本地备份
          </Button>
          <Button type="button" disabled={operation !== null} onClick={sync}>
            {operation === 'sync' ? <Spinner data-icon="inline-start" /> : <RefreshCw data-icon="inline-start" />}
            立即同步
          </Button>
        </div>
      </div>

      <FieldGroup>
        <div className="grid grid-cols-1 gap-5 lg:grid-cols-2">
          <Field>
            <FieldLabel htmlFor="webdav-endpoint">WebDAV 地址</FieldLabel>
            <Input id="webdav-endpoint" type="url" value={draft.endpoint} onChange={event => setDraft(current => ({ ...current, endpoint: event.target.value }))} placeholder="https://dav.example.com/remote.php/dav/files/user" />
          </Field>
          <Field>
            <FieldLabel htmlFor="webdav-path">远端目录</FieldLabel>
            <Input id="webdav-path" value={draft.remotePath} onChange={event => setDraft(current => ({ ...current, remotePath: event.target.value }))} />
          </Field>
          <Field>
            <FieldLabel htmlFor="webdav-username">用户名</FieldLabel>
            <Input id="webdav-username" autoComplete="username" value={draft.username} onChange={event => setDraft(current => ({ ...current, username: event.target.value }))} />
          </Field>
          <Field>
            <FieldLabel htmlFor="webdav-password">密码</FieldLabel>
            <Input id="webdav-password" type="password" autoComplete="current-password" value={draft.password} onChange={event => setDraft(current => ({ ...current, password: event.target.value }))} placeholder={settings?.passwordConfigured ? '已保存，留空保持不变' : ''} />
          </Field>
          <Field>
            <FieldLabel htmlFor="webdav-interval">自动同步间隔（小时）</FieldLabel>
            <Input id="webdav-interval" type="number" min={1} max={720} value={draft.intervalHours} onChange={event => setDraft(current => ({ ...current, intervalHours: event.target.valueAsNumber || 1 }))} />
          </Field>
          <Field>
            <FieldLabel htmlFor="backup-retention">本地保留份数</FieldLabel>
            <Input id="backup-retention" type="number" min={1} max={100} value={draft.retention} onChange={event => setDraft(current => ({ ...current, retention: event.target.valueAsNumber || 1 }))} />
          </Field>
        </div>
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
          <FieldLabel htmlFor="webdav-enabled">
            <Field orientation="horizontal">
              <FieldContent>
                <FieldTitle>启用 WebDAV</FieldTitle>
                <FieldDescription>允许手动同步和远端恢复。</FieldDescription>
              </FieldContent>
              <Switch id="webdav-enabled" checked={draft.enabled} onCheckedChange={enabled => setDraft(current => ({ ...current, enabled }))} />
            </Field>
          </FieldLabel>
          <FieldLabel htmlFor="webdav-auto-sync">
            <Field orientation="horizontal" data-disabled={!draft.enabled}>
              <FieldContent>
                <FieldTitle>自动备份</FieldTitle>
                <FieldDescription>按配置间隔生成并上传最新备份。</FieldDescription>
              </FieldContent>
              <Switch id="webdav-auto-sync" disabled={!draft.enabled} checked={draft.autoSync} onCheckedChange={autoSync => setDraft(current => ({ ...current, autoSync }))} />
            </Field>
          </FieldLabel>
        </div>
        <div className="flex flex-wrap justify-end gap-2">
          <Button type="button" variant="outline" disabled={operation !== null} onClick={() => setConfirmRemoteRestore(true)}>
            <RotateCcw data-icon="inline-start" />
            从 WebDAV 恢复
          </Button>
          <Button type="button" disabled={operation !== null} onClick={save}>
            {operation === 'save' && <Spinner data-icon="inline-start" />}
            保存设置
          </Button>
        </div>
      </FieldGroup>

      <Separator className="my-8" />
      <div className="mb-3 flex items-center justify-between">
        <h3 className="text-sm font-semibold">本地备份</h3>
        <span className="text-xs text-muted-foreground">{backups.length} 份</span>
      </div>
      {backups.length === 0 ? (
        <Empty>
          <EmptyHeader>
            <EmptyMedia variant="icon"><DatabaseBackup /></EmptyMedia>
            <EmptyTitle>暂无备份</EmptyTitle>
            <EmptyDescription>创建后可在本机恢复或同步到 WebDAV。</EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        <div className="divide-y">
          {backups.map(backup => (
            <div key={backup.name} className="flex flex-col gap-3 py-3 sm:flex-row sm:items-center">
              <div className="min-w-0 flex-1">
                <p className="truncate font-mono text-sm" title={backup.name}>{backup.name}</p>
                <p className="mt-1 text-xs text-muted-foreground">{backup.createdAt} · {formatSize(backup.size)}</p>
              </div>
              <div className="flex gap-2">
                <Button type="button" size="sm" variant="outline" disabled={operation !== null} onClick={() => setRestoreTarget(backup)}>
                  <RotateCcw data-icon="inline-start" />
                  恢复
                </Button>
                <Button type="button" size="icon" variant="ghost" title="删除备份" disabled={operation !== null} onClick={() => setDeleteTarget(backup)}>
                  {operation === `delete:${backup.name}` ? <Spinner /> : <Trash2 />}
                  <span className="sr-only">删除备份</span>
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}

      <AlertDialog open={restoreTarget !== null} onOpenChange={open => { if (!open) setRestoreTarget(null); }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>恢复本地备份？</AlertDialogTitle>
            <AlertDialogDescription>当前内核会停止，数据库与规则集源快照将恢复到所选版本。</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction onClick={restoreLocal}>确认恢复</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog open={deleteTarget !== null} onOpenChange={open => { if (!open) setDeleteTarget(null); }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>删除本地备份？</AlertDialogTitle>
            <AlertDialogDescription>{deleteTarget?.name} 将从本机永久删除。</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction onClick={() => { if (deleteTarget) remove(deleteTarget); }}>
              确认删除
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog open={confirmRemoteRestore} onOpenChange={setConfirmRemoteRestore}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>从 WebDAV 恢复？</AlertDialogTitle>
            <AlertDialogDescription>将下载 latest.zip，并替换当前数据库与规则集源快照。</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction onClick={restoreRemote}>确认恢复</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  );
}
