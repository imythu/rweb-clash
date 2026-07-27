import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  ArrowDown,
  ArrowUp,
  CircleX,
  Download,
  Network,
  SearchX,
  Upload,
  X,
} from 'lucide-react';
import { api, type Connection, type Traffic } from '@/lib/api';
import { usePageActivity } from '@/lib/usePageActivity';
import { useToast } from './toast-context';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/components/ui/alert-dialog';
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty';
import { Separator } from '@/components/ui/separator';
import { Skeleton } from '@/components/ui/skeleton';

const CONNECTION_POLL_MS = 1_000;

type LiveConnection = Connection & {
  uploadRate: number;
  downloadRate: number;
};

type PreviousSample = {
  upload: number;
  download: number;
  sampledAt: number;
};

type SortKey = 'speed' | 'download' | 'upload' | 'domain' | 'process' | 'start';

const SORT_OPTIONS: Array<{ value: SortKey; label: string }> = [
  { value: 'speed', label: '实时速率' },
  { value: 'download', label: '下载流量' },
  { value: 'upload', label: '上传流量' },
  { value: 'domain', label: '目标地址' },
  { value: 'process', label: '进程' },
  { value: 'start', label: '建立时间' },
];

function formatBytes(bytes: number) {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${bytes} B`;
}

function formatRate(bytes: number) {
  return `${formatBytes(bytes)}/s`;
}

function targetLabel(connection: Connection) {
  const destination = [connection.destinationIp, connection.destinationPort]
    .filter(Boolean)
    .join(':');
  return connection.domain || destination || '未知目标';
}

function connectionSearchText(connection: Connection) {
  return [
    targetLabel(connection),
    connection.process,
    connection.sourceIp,
    connection.sourcePort,
    connection.destinationIp,
    connection.destinationPort,
    connection.rule,
    connection.rulePayload,
    connection.policy,
    connection.network,
    connection.type,
    ...connection.chains,
  ]
    .filter(Boolean)
    .join(' ')
    .toLocaleLowerCase();
}

function DetailRow({ label, value }: { label: string; value?: string | number | null }) {
  return (
    <div className="grid grid-cols-[7rem_minmax(0,1fr)] items-start gap-4 py-3 text-sm">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="min-w-0 break-all font-mono text-foreground">{value || '-'}</dd>
    </div>
  );
}

export function Connections() {
  const { toast } = useToast();
  const isPageActive = usePageActivity();
  const [connections, setConnections] = useState<LiveConnection[]>([]);
  const [traffic, setTraffic] = useState<Traffic>({ up: 0, down: 0 });
  const [search, setSearch] = useState('');
  const [sortKey, setSortKey] = useState<SortKey>('speed');
  const [ascending, setAscending] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [closing, setClosing] = useState<Set<string>>(new Set());
  const [closingAll, setClosingAll] = useState(false);
  const previousSamples = useRef(new Map<string, PreviousSample>());
  const inFlight = useRef(false);
  const closingIds = useRef(new Set<string>());
  const closeAllInFlight = useRef(false);

  const refresh = useCallback(async () => {
    if (!isPageActive || document.hidden || inFlight.current) return;
    inFlight.current = true;
    try {
      const [nextConnections, nextTraffic] = await Promise.all([
        api.connections(),
        api.traffic(),
      ]);
      const sampledAt = performance.now();
      const nextSamples = new Map<string, PreviousSample>();
      const live = nextConnections.map(connection => {
        const previous = previousSamples.current.get(connection.id);
        const elapsed = previous ? Math.max((sampledAt - previous.sampledAt) / 1_000, 0.001) : 0;
        const uploadRate = previous
          ? Math.max(0, connection.upload - previous.upload) / elapsed
          : 0;
        const downloadRate = previous
          ? Math.max(0, connection.download - previous.download) / elapsed
          : 0;
        nextSamples.set(connection.id, {
          upload: connection.upload,
          download: connection.download,
          sampledAt,
        });
        return { ...connection, uploadRate, downloadRate };
      });
      previousSamples.current = nextSamples;
      setConnections(live);
      setTraffic(nextTraffic);
    } catch {
      if (loading) toast('连接数据加载失败', 'error');
    } finally {
      inFlight.current = false;
      setLoading(false);
    }
  }, [isPageActive, loading, toast]);

  useEffect(() => {
    void refresh();
    const interval = window.setInterval(() => void refresh(), CONNECTION_POLL_MS);
    const handleVisibility = () => {
      if (!document.hidden) void refresh();
    };
    document.addEventListener('visibilitychange', handleVisibility);
    return () => {
      window.clearInterval(interval);
      document.removeEventListener('visibilitychange', handleVisibility);
    };
  }, [refresh]);

  const visibleConnections = useMemo(() => {
    const query = search.trim().toLocaleLowerCase();
    const filtered = query
      ? connections.filter(connection => connectionSearchText(connection).includes(query))
      : connections;
    const direction = ascending ? 1 : -1;
    return [...filtered].sort((left, right) => {
      switch (sortKey) {
        case 'speed':
          return direction * ((left.downloadRate + left.uploadRate) - (right.downloadRate + right.uploadRate));
        case 'download':
          return direction * (left.download - right.download);
        case 'upload':
          return direction * (left.upload - right.upload);
        case 'domain':
          return direction * targetLabel(left).localeCompare(targetLabel(right));
        case 'process':
          return direction * (left.process || '').localeCompare(right.process || '');
        case 'start':
          return direction * (left.start || '').localeCompare(right.start || '');
      }
    });
  }, [ascending, connections, search, sortKey]);

  const selected = connections.find(connection => connection.id === selectedId) ?? null;
  const totals = useMemo(() => connections.reduce(
    (sum, connection) => ({
      upload: sum.upload + connection.upload,
      download: sum.download + connection.download,
    }),
    { upload: 0, download: 0 },
  ), [connections]);

  const closeConnection = async (id: string) => {
    if (closingIds.current.has(id) || closeAllInFlight.current) return;
    closingIds.current.add(id);
    setClosing(current => new Set(current).add(id));
    try {
      await api.closeConnection(id);
      setConnections(current => current.filter(connection => connection.id !== id));
      if (selectedId === id) setSelectedId(null);
      toast('连接已关闭', 'success');
    } catch {
      toast('关闭连接失败', 'error');
    } finally {
      closingIds.current.delete(id);
      setClosing(current => {
        const next = new Set(current);
        next.delete(id);
        return next;
      });
    }
  };

  const closeAll = async () => {
    if (closeAllInFlight.current) return;
    closeAllInFlight.current = true;
    setClosingAll(true);
    try {
      await api.closeAllConnections();
      previousSamples.current.clear();
      setConnections([]);
      setSelectedId(null);
      toast('全部连接已关闭', 'success');
    } catch {
      toast('关闭全部连接失败', 'error');
    } finally {
      closeAllInFlight.current = false;
      setClosingAll(false);
    }
  };

  return (
    <div className="mx-auto flex w-full max-w-[1600px] flex-col gap-5 pb-16">
      <div className="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
        <div>
          <h2 className="text-2xl font-semibold">实时连接</h2>
          <p className="mt-1 text-sm text-muted-foreground">{connections.length} 个活跃连接</p>
        </div>
        <AlertDialog>
          <AlertDialogTrigger asChild>
            <Button variant="destructive" disabled={connections.length === 0 || closingAll}>
              <CircleX data-icon="inline-start" />
              关闭全部
            </Button>
          </AlertDialogTrigger>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>关闭全部连接？</AlertDialogTitle>
              <AlertDialogDescription>
                当前 {connections.length} 个连接会立即中断，新请求仍可重新建立连接。
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>取消</AlertDialogCancel>
              <AlertDialogAction onClick={() => void closeAll()}>确认关闭</AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </div>

      <section className="grid grid-cols-2 border-y bg-card sm:grid-cols-4">
        <div className="flex min-h-24 flex-col justify-center gap-1 border-b p-4 sm:border-b-0 sm:border-r">
          <span className="text-xs text-muted-foreground">实时下载</span>
          <strong className="font-mono text-lg">{formatRate(traffic.down)}</strong>
        </div>
        <div className="flex min-h-24 flex-col justify-center gap-1 border-b p-4 sm:border-b-0 sm:border-r">
          <span className="text-xs text-muted-foreground">实时上传</span>
          <strong className="font-mono text-lg">{formatRate(traffic.up)}</strong>
        </div>
        <div className="flex min-h-24 flex-col justify-center gap-1 border-r p-4">
          <span className="text-xs text-muted-foreground">累计下载</span>
          <strong className="font-mono text-lg">{formatBytes(totals.download)}</strong>
        </div>
        <div className="flex min-h-24 flex-col justify-center gap-1 p-4">
          <span className="text-xs text-muted-foreground">累计上传</span>
          <strong className="font-mono text-lg">{formatBytes(totals.upload)}</strong>
        </div>
      </section>

      <div className="flex flex-col gap-3 border-b pb-4 md:flex-row md:items-center">
        <Input
          value={search}
          onChange={event => setSearch(event.target.value)}
          placeholder="搜索域名、IP、进程、规则或策略"
          aria-label="搜索连接"
          className="md:max-w-md"
        />
        <div className="flex gap-2 md:ml-auto">
          <Select value={sortKey} onValueChange={value => setSortKey(value as SortKey)}>
            <SelectTrigger aria-label="连接排序字段" className="min-w-36">
              <SelectValue placeholder="排序字段" />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                {SORT_OPTIONS.map(option => (
                  <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
          <Button
            variant="outline"
            size="icon"
            title={ascending ? '升序' : '降序'}
            onClick={() => setAscending(value => !value)}
          >
            {ascending ? <ArrowUp /> : <ArrowDown />}
            <span className="sr-only">切换排序方向</span>
          </Button>
        </div>
      </div>

      {loading ? (
        <div className="flex flex-col gap-3">
          {Array.from({ length: 6 }, (_, index) => <Skeleton key={index} className="h-14 w-full" />)}
        </div>
      ) : visibleConnections.length === 0 ? (
        <Empty>
          <EmptyHeader>
            <EmptyMedia variant="icon">{search ? <SearchX /> : <Network />}</EmptyMedia>
            <EmptyTitle>{search ? '没有匹配的连接' : '当前没有活跃连接'}</EmptyTitle>
            <EmptyDescription>{search ? '请调整搜索条件。' : '连接建立后会在此实时显示。'}</EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        <>
          <div className="hidden overflow-hidden rounded-lg border md:block">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>目标</TableHead>
                  <TableHead>进程</TableHead>
                  <TableHead>策略链</TableHead>
                  <TableHead className="text-right">实时速率</TableHead>
                  <TableHead className="text-right">累计流量</TableHead>
                  <TableHead className="w-14"><span className="sr-only">操作</span></TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {visibleConnections.map(connection => (
                  <TableRow
                    key={connection.id}
                    tabIndex={0}
                    role="button"
                    onClick={() => setSelectedId(connection.id)}
                    onKeyDown={event => {
                      if (event.key === 'Enter' || event.key === ' ') setSelectedId(connection.id);
                    }}
                    className="cursor-pointer"
                  >
                    <TableCell className="max-w-72">
                      <div className="flex min-w-0 flex-col gap-1">
                        <span className="truncate font-medium" title={targetLabel(connection)}>{targetLabel(connection)}</span>
                        <span className="font-mono text-xs text-muted-foreground">
                          {[connection.network, connection.type].filter(Boolean).join(' / ') || '-'}
                        </span>
                      </div>
                    </TableCell>
                    <TableCell className="max-w-52 truncate" title={connection.process || ''}>{connection.process || '-'}</TableCell>
                    <TableCell className="max-w-64">
                      <div className="flex flex-wrap gap-1">
                        {(connection.chains.length ? connection.chains : [connection.policy || 'DIRECT']).map(chain => (
                          <Badge key={chain} variant="outline">{chain}</Badge>
                        ))}
                      </div>
                    </TableCell>
                    <TableCell className="text-right font-mono">
                      <div className="flex flex-col gap-1">
                        <span>↓ {formatRate(connection.downloadRate)}</span>
                        <span className="text-xs text-muted-foreground">↑ {formatRate(connection.uploadRate)}</span>
                      </div>
                    </TableCell>
                    <TableCell className="text-right font-mono">
                      <div className="flex flex-col gap-1">
                        <span>{formatBytes(connection.download)}</span>
                        <span className="text-xs text-muted-foreground">{formatBytes(connection.upload)}</span>
                      </div>
                    </TableCell>
                    <TableCell>
                      <Button
                        variant="ghost"
                        size="icon"
                        title="关闭连接"
                        disabled={closing.has(connection.id)}
                        onClick={event => {
                          event.stopPropagation();
                          void closeConnection(connection.id);
                        }}
                      >
                        <X />
                        <span className="sr-only">关闭连接</span>
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>

          <div className="flex flex-col md:hidden">
            {visibleConnections.map((connection, index) => (
              <div key={connection.id}>
                {index > 0 && <Separator />}
                <button
                  type="button"
                  onClick={() => setSelectedId(connection.id)}
                  className="flex w-full min-w-0 items-start gap-3 py-4 text-left"
                >
                  <div className="min-w-0 flex-1">
                    <p className="break-all text-sm font-medium">{targetLabel(connection)}</p>
                    <p className="mt-1 truncate text-xs text-muted-foreground">{connection.process || connection.policy || '-'}</p>
                    <div className="mt-2 flex flex-wrap gap-1">
                      <Badge variant="outline">{connection.network || '未知网络'}</Badge>
                      <Badge variant="secondary">{connection.policy || 'DIRECT'}</Badge>
                    </div>
                  </div>
                  <div className="shrink-0 text-right font-mono text-xs">
                    <p>↓ {formatRate(connection.downloadRate)}</p>
                    <p className="mt-1 text-muted-foreground">↑ {formatRate(connection.uploadRate)}</p>
                  </div>
                </button>
              </div>
            ))}
          </div>
        </>
      )}

      <Sheet open={selected !== null} onOpenChange={open => { if (!open) setSelectedId(null); }}>
        <SheetContent className="flex w-full flex-col gap-0 overflow-y-auto sm:max-w-xl">
          {selected && (
            <>
              <SheetHeader className="pr-8">
                <SheetTitle>连接详情</SheetTitle>
                <SheetDescription className="flex flex-col gap-1">
                  <span className="break-all font-mono text-foreground">{targetLabel(selected)}</span>
                  <span>{selected.process || '未知进程'}</span>
                </SheetDescription>
              </SheetHeader>
              <div className="mt-6 grid grid-cols-2 gap-3">
                <div className="rounded-lg border p-3">
                  <Download className="text-muted-foreground" />
                  <p className="mt-2 font-mono text-lg">{formatRate(selected.downloadRate)}</p>
                  <p className="text-xs text-muted-foreground">累计 {formatBytes(selected.download)}</p>
                </div>
                <div className="rounded-lg border p-3">
                  <Upload className="text-muted-foreground" />
                  <p className="mt-2 font-mono text-lg">{formatRate(selected.uploadRate)}</p>
                  <p className="text-xs text-muted-foreground">累计 {formatBytes(selected.upload)}</p>
                </div>
              </div>
              <dl className="mt-6 divide-y">
                <DetailRow label="连接 ID" value={selected.id} />
                <DetailRow label="源地址" value={[selected.sourceIp, selected.sourcePort].filter(Boolean).join(':')} />
                <DetailRow label="目标地址" value={[selected.destinationIp, selected.destinationPort].filter(Boolean).join(':')} />
                <DetailRow label="网络 / 类型" value={[selected.network, selected.type].filter(Boolean).join(' / ')} />
                <DetailRow label="命中规则" value={selected.rule} />
                <DetailRow label="规则内容" value={selected.rulePayload} />
                <DetailRow label="代理链" value={selected.chains.join(' → ')} />
                <DetailRow label="建立时间" value={selected.start} />
              </dl>
              <Button
                variant="destructive"
                className="mt-6"
                disabled={closing.has(selected.id)}
                onClick={() => void closeConnection(selected.id)}
              >
                <CircleX data-icon="inline-start" />
                关闭连接
              </Button>
            </>
          )}
        </SheetContent>
      </Sheet>
    </div>
  );
}
