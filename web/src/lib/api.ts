export type SystemConfig = {
  allow_lan: boolean;
  ipv6: boolean;
  log_level: 'silent' | 'error' | 'warning' | 'info' | 'debug';
  mixed_port: number;
  external_controller: string;
  external_controller_enabled: boolean;
  secret: string;
  dns_enabled: boolean;
  dns_mode: 'fake-ip' | 'redir-host';
  dns_nameservers: string[];
  dns_fallback: string[];
  dns_fake_ip_filter: string[];
  dns_nameserver_policy: Record<string, string[]>;
  dns_hosts: Record<string, string[]>;
  store_selected: boolean;
  unified_delay: boolean;
  tcp_concurrent: boolean;
  tun: boolean;
  system_proxy: boolean;
  mode: 'rule' | 'global' | 'direct';
  auto_start: boolean;
};

export type CoreStatus = {
  state: string;
  pid: number | null;
  started_at: string | null;
  last_error: string | null;
  controller_addr: string;
  version: string | null;
};

export type SystemStatus = {
  core: CoreStatus;
  config: SystemConfig;
};

export type SetupStatus = {
  needsOnboarding: boolean;
  hasSubscriptions: boolean;
  subscriptionCount: number;
  hasSources: boolean;
  manualNodeCount: number;
  coreReady: boolean;
  corePath: string;
  mixedPortAvailable: boolean;
  controllerPortAvailable: boolean;
  warnings: string[];
};

export type Egress = {
  ip: string | null;
  provider: string | null;
  country: string | null;
  source: string | null;
};

export type Traffic = {
  up: number;
  down: number;
};

export type Connection = {
  id: string;
  domain: string | null;
  rule: string | null;
  policy: string | null;
  speed: string;
  network: string | null;
  type: string | null;
  sourceIp: string | null;
  sourcePort: string | null;
  destinationIp: string | null;
  destinationPort: string | null;
  process: string | null;
  start: string | null;
  upload: number;
  download: number;
  chains: string[];
  rulePayload: string | null;
};

export type DownloadRoute = 'direct' | 'core' | 'system' | 'auto';

export type FilterRule = {
  id: string;
  action: 'keep' | 'discard' | string;
  type: string;
  pattern: string;
  values?: string[];
  enabled: boolean;
};

export type FilterRuleInput = Omit<FilterRule, 'id' | 'enabled'> & {
  id?: string;
  enabled?: boolean;
};

export type Subscription = {
  id: string;
  name: string;
  url: string;
  format: string;
  nodes: number;
  status: string;
  traffic: {
    used: number;
    total: number;
  };
  expiry: string | null;
  intervalSeconds: number;
  interval: number;
  inheritGlobal: boolean;
  rules: FilterRule[];
  breakdown: Record<string, number>;
  lastUpdate: string | null;
  lastError: string | null;
  downloadRoute: DownloadRoute;
  lastRoute: string | null;
};

export type SubscriptionMemberNode = {
  name: string;
  displayName: string;
  protocol: string;
  country: string | null;
  latency: number;
  filteredOut: boolean;
  filterReason: string | null;
};

export type SubscriptionMemberGroup = {
  name: string;
  displayName: string;
  type: string;
  members: string[];
  memberCount: number;
  filteredOut: boolean;
  filterReason: string | null;
};

export type SubscriptionMemberSection = {
  nodes: SubscriptionMemberNode[];
  groups: SubscriptionMemberGroup[];
};

export type SubscriptionMembers = {
  subscriptionId: string;
  subscriptionName: string;
  filtered: SubscriptionMemberSection;
  beforeFilter: SubscriptionMemberSection;
};

export type SubscriptionInput = {
  name: string;
  url: string;
  format?: string;
  interval?: number;
  intervalSeconds?: number;
  inheritGlobal?: boolean;
  rules?: FilterRuleInput[];
  downloadRoute?: DownloadRoute;
};

export type GroupFilter = {
  id?: string;
  action: 'keep' | 'discard' | string;
  type: string;
  operator: string;
  value?: string;
  values?: string[];
  enabled?: boolean;
};

export type ProxyGroup = {
  name: string;
  displayName: string;
  type: string;
  source: string;
  builtin: boolean;
  subscriptionName: string | null;
  now: string | null;
  delay: number;
  all: string[];
  filter: GroupFilter[];
};

export type ProxyNode = {
  name: string;
  displayName: string;
  type: string;
  latency: number;
  country: string | null;
  subscriptionId: string | null;
  subscriptionName: string | null;
};

export type ProxyTopology = {
  groups: ProxyGroup[];
  nodes: ProxyNode[];
};

export type ProxyGroupInput = {
  name: string;
  type: string;
  filter: GroupFilter[];
};

export type DelayResult = {
  name: string;
  delay: number;
};

export type Rule = {
  id: string;
  type: string;
  value: string;
  policy: string;
  position: number;
  source: string;
  enabled: boolean;
  desc: string | null;
};

export type RuleInput = {
  type: string;
  value: string;
  policy: string;
  desc?: string | null;
  enabled?: boolean;
  position?: number;
};

export type RuleSetBehavior = 'domain' | 'ipcidr' | 'classical';

export type RuleSet = {
  id: string;
  name: string;
  url: string;
  behavior: RuleSetBehavior | null;
  format: string;
  ruleCount: number;
  lastUpdate: string | null;
  lastError: string | null;
  downloadRoute: DownloadRoute;
  lastRoute: string | null;
};

export type RuleSetInput = {
  name: string;
  url: string;
  interval?: number;
  intervalSeconds?: number;
  behavior?: RuleSetBehavior;
  format?: string;
  downloadRoute?: DownloadRoute;
};

export type ManualNode = {
  name: string;
  displayName: string;
  type: string;
  config: Record<string, unknown>;
  latency: number;
};

export type ManualNodeInput = {
  name: string;
  config: Record<string, unknown>;
};

export type WebDavSettings = {
  endpoint: string;
  username: string;
  passwordConfigured: boolean;
  remotePath: string;
  enabled: boolean;
  autoSync: boolean;
  intervalHours: number;
  retention: number;
  lastSync: string | null;
  lastError: string | null;
};

export type WebDavSettingsInput = {
  endpoint: string;
  username: string;
  password?: string | null;
  remotePath: string;
  enabled: boolean;
  autoSync: boolean;
  intervalHours: number;
  retention: number;
};

export type Backup = {
  name: string;
  size: number;
  createdAt: string;
  remoteAvailable: boolean;
};

export type RuleTestResult = {
  hitRule: Rule;
  finalProxy: string;
};

export type LogEntry = {
  time: string;
  level: 'info' | 'warning' | 'error' | 'debug' | string;
  payload: string;
  parsedHost?: string | null;
};

export type OperationResponse = {
  success: boolean;
  message: string;
};

type QueryValue = string | number | boolean | null | undefined;

type ApiRequestOptions = Omit<RequestInit, 'body'> & {
  json?: unknown;
  query?: Record<string, QueryValue>;
};

type ErrorEnvelope = {
  error?: {
    code?: string;
    message?: string;
    trace_id?: string;
  };
};

export class ApiError extends Error {
  status: number;
  code: string;
  traceId: string;

  constructor(status: number, code: string, message: string, traceId: string) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.code = code;
    this.traceId = traceId;
  }
}

const API_TOKEN_STORAGE_KEY = 'rweb-clash:api-token';

function writeStoredApiToken(token: string) {
  try {
    if (token) localStorage.setItem(API_TOKEN_STORAGE_KEY, token);
    else localStorage.removeItem(API_TOKEN_STORAGE_KEY);
  } catch {
    // Storage can be unavailable in hardened or private browser contexts.
  }
}

function readInitialApiToken() {
  if (typeof window === 'undefined') return '';

  const hash = window.location.hash.startsWith('#') ? window.location.hash.slice(1) : window.location.hash;
  const hashParams = new URLSearchParams(hash);
  if (hashParams.has('token')) {
    const token = hashParams.get('token')?.trim() ?? '';
    hashParams.delete('token');
    const remainingHash = hashParams.toString();
    const sanitizedUrl = `${window.location.pathname}${window.location.search}${remainingHash ? `#${remainingHash}` : ''}`;
    window.history.replaceState(window.history.state, document.title, sanitizedUrl);
    writeStoredApiToken(token);
    return token;
  }

  try {
    return localStorage.getItem(API_TOKEN_STORAGE_KEY)?.trim() ?? '';
  } catch {
    return '';
  }
}

let apiToken = readInitialApiToken();

export function getApiToken() {
  return apiToken;
}

export function setApiToken(token: string) {
  apiToken = token.trim();
  writeStoredApiToken(apiToken);
}

export function clearApiToken() {
  apiToken = '';
  writeStoredApiToken('');
}

const API_BASE = import.meta.env.VITE_API_BASE_URL ?? '/api';

function buildUrl(path: string, query?: Record<string, QueryValue>) {
  const base = API_BASE.endsWith('/') ? API_BASE.slice(0, -1) : API_BASE;
  const normalizedPath = path.startsWith('/') ? path : `/${path}`;
  const url = new URL(`${base}${normalizedPath}`, window.location.origin);

  Object.entries(query ?? {}).forEach(([key, value]) => {
    if (value !== undefined && value !== null && value !== '') {
      url.searchParams.set(key, String(value));
    }
  });

  return url.toString();
}

async function readError(response: Response) {
  const traceId = response.headers.get('x-trace-id') ?? '';
  try {
    const envelope = (await response.json()) as ErrorEnvelope;
    return new ApiError(
      response.status,
      envelope.error?.code ?? 'request_failed',
      envelope.error?.message ?? response.statusText,
      envelope.error?.trace_id ?? traceId,
    );
  } catch {
    return new ApiError(response.status, 'request_failed', response.statusText, traceId);
  }
}

async function request<T>(path: string, options: ApiRequestOptions = {}): Promise<T> {
  const { json, query, headers, ...init } = options;
  const requestHeaders = new Headers(headers);
  requestHeaders.set('Accept', 'application/json');
  if (apiToken) requestHeaders.set('Authorization', `Bearer ${apiToken}`);

  const requestInit: RequestInit = {
    ...init,
    headers: requestHeaders,
  };

  if (json !== undefined) {
    requestHeaders.set('Content-Type', 'application/json');
    requestInit.body = JSON.stringify(json);
  }

  const response = await fetch(buildUrl(path, query), requestInit);
  if (!response.ok) {
    throw await readError(response);
  }

  if (response.status === 204) {
    return undefined as T;
  }

  const contentType = response.headers.get('content-type') ?? '';
  if (contentType.includes('application/json')) {
    return response.json() as Promise<T>;
  }

  return response.text() as Promise<T>;
}

function encodePathPart(value: string) {
  return encodeURIComponent(value);
}

export const api = {
  getConfig: () => request<SystemConfig>('/configs'),
  patchConfig: (patch: Partial<SystemConfig>) =>
    request<SystemConfig>('/configs', { method: 'PATCH', json: patch }),

  setupStatus: () => request<SetupStatus>('/setup/status'),
  systemStatus: (signal?: AbortSignal) => request<SystemStatus>('/system/status', { signal }),
  systemEgress: (signal?: AbortSignal) => request<Egress>('/system/egress', { signal }),
  coreStatus: () => request<CoreStatus>('/core/status'),
  startCore: () => request<CoreStatus>('/core/start', { method: 'POST' }),
  stopCore: () => request<CoreStatus>('/core/stop', { method: 'POST' }),
  restartCore: () => request<CoreStatus>('/core/restart', { method: 'POST' }),

  listSubscriptions: () => request<Subscription[]>('/subscriptions'),
  subscriptionMembers: (id: string) =>
    request<SubscriptionMembers>(`/subscriptions/${encodePathPart(id)}/members`),
  createSubscription: (input: SubscriptionInput) =>
    request<Subscription[]>('/subscriptions', { method: 'POST', json: input }),
  updateSubscription: (id: string, input: SubscriptionInput) =>
    request<Subscription[]>(`/subscriptions/${encodePathPart(id)}`, { method: 'PATCH', json: input }),
  deleteSubscription: (id: string) =>
    request<void>(`/subscriptions/${encodePathPart(id)}`, { method: 'DELETE' }),
  refreshSubscription: (id: string) =>
    request<OperationResponse>(`/subscriptions/${encodePathPart(id)}/refresh`, { method: 'POST' }),
  listGlobalFilterRules: () => request<FilterRule[]>('/subscription-rules/global'),
  replaceGlobalFilterRules: (rules: FilterRuleInput[]) =>
    request<FilterRule[]>('/subscription-rules/global', { method: 'PUT', json: rules }),

  proxyTopology: () => request<ProxyTopology>('/proxies'),
  createProxyGroup: (input: ProxyGroupInput) =>
    request<void>('/proxies', { method: 'POST', json: input }),
  updateProxyGroup: (group: string, input: ProxyGroupInput) =>
    request<OperationResponse>(`/proxies/${encodePathPart(group)}`, { method: 'PUT', json: input }),
  deleteProxyGroup: (group: string) =>
    request<void>(`/proxies/${encodePathPart(group)}`, { method: 'DELETE' }),
  selectProxy: (group: string, name: string) =>
    request<OperationResponse>(`/proxies/${encodePathPart(group)}`, { method: 'PUT', json: { name } }),
  testProxyGroup: (group: string) =>
    request<DelayResult[]>(`/proxies/${encodePathPart(group)}/test`, { method: 'POST' }),
  testNode: (name: string) =>
    request<DelayResult>('/nodes/test', { method: 'POST', json: { name } }),
  listManualNodes: () => request<ManualNode[]>('/manual-nodes'),
  createManualNode: (input: ManualNodeInput) =>
    request<ManualNode[]>('/manual-nodes', { method: 'POST', json: input }),
  updateManualNode: (name: string, input: ManualNodeInput) =>
    request<ManualNode[]>(`/manual-nodes/${encodePathPart(name)}`, { method: 'PUT', json: input }),
  deleteManualNode: (name: string) =>
    request<void>(`/manual-nodes/${encodePathPart(name)}`, { method: 'DELETE' }),

  listRules: () => request<Rule[]>('/rules'),
  createRule: (input: RuleInput) => request<Rule>('/rules', { method: 'POST', json: input }),
  updateRule: (id: string, input: RuleInput) =>
    request<Rule>(`/rules/${encodePathPart(id)}`, { method: 'PUT', json: input }),
  deleteRule: (id: string) => request<void>(`/rules/${encodePathPart(id)}`, { method: 'DELETE' }),
  testRule: (target: string) =>
    request<RuleTestResult>('/rules/test', { method: 'POST', json: { target } }),

  listRuleSets: () => request<RuleSet[]>('/rule-sets'),
  createRuleSet: (input: RuleSetInput) =>
    request<RuleSet>('/rule-sets', { method: 'POST', json: input }),
  refreshRuleSet: (id: string) =>
    request<OperationResponse>(`/rule-sets/${encodePathPart(id)}/refresh`, { method: 'POST' }),
  deleteRuleSet: (id: string) => request<void>(`/rule-sets/${encodePathPart(id)}`, { method: 'DELETE' }),

  listLogs: (query?: { level?: string; search?: string }) => request<LogEntry[]>('/logs', { query }),
  clearLogs: () => request<void>('/logs', { method: 'DELETE' }),
  exportLogs: () => request<string>('/logs/export'),
  exportDiagnostics: () => request<string>('/diagnostics/export'),

  listBackups: () => request<Backup[]>('/backups'),
  createBackup: () => request<Backup>('/backups', { method: 'POST' }),
  deleteBackup: (name: string) => request<void>(`/backups/${encodePathPart(name)}`, { method: 'DELETE' }),
  restoreBackup: (name: string) =>
    request<OperationResponse>(`/backups/${encodePathPart(name)}/restore`, { method: 'POST' }),
  webdavSettings: () => request<WebDavSettings>('/webdav'),
  saveWebdavSettings: (input: WebDavSettingsInput) =>
    request<WebDavSettings>('/webdav', { method: 'PUT', json: input }),
  testWebdav: () => request<OperationResponse>('/webdav/test', { method: 'POST' }),
  syncWebdav: () => request<Backup>('/webdav/sync', { method: 'POST' }),
  restoreWebdav: () => request<OperationResponse>('/webdav/restore', { method: 'POST' }),

  traffic: () => request<Traffic>('/traffic'),
  connections: () => request<Connection[]>('/connections'),
  closeConnection: (id: string) => request<void>(`/connections/${encodePathPart(id)}`, { method: 'DELETE' }),
  closeAllConnections: () => request<void>('/connections', { method: 'DELETE' }),
  flushDns: () => request<void>('/dns/flush', { method: 'POST' }),
};
