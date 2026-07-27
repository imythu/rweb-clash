// Schema source: MetaCubeX/Meta-Docs commit 4a5648a112f7492829d56feb77051c696d84f514 (2026-07-23).
// https://wiki.metacubex.one/config/proxies/
// Keep this file declarative: the manual-node editor owns rendering and serialization.

export type ManualNodeFieldKind =
  | 'text'
  | 'password'
  | 'number'
  | 'textarea'
  | 'boolean'
  | 'select'
  | 'string-list'
  | 'number-list'
  | 'key-value-list'
  | 'object-list';

export type ManualNodeConditionValue = string | number | boolean;

export type ManualNodeShowWhen = {
  key: string;
  equals?: ManualNodeConditionValue;
  oneOf?: ManualNodeConditionValue[];
  truthy?: boolean;
};

export type ManualNodeField = {
  key: string;
  label: string;
  section: string;
  kind?: ManualNodeFieldKind;
  required?: boolean;
  placeholder?: string;
  description?: string;
  options?: Array<{ value: string; label: string }>;
  /** Output normalization for controls whose DOM value is textual, notably select. */
  valueType?: 'string' | 'number';
  showWhen?: ManualNodeShowWhen;
  min?: number;
  max?: number;
  step?: number;
  minItems?: number;
  maxItems?: number;
  valueKind?: 'text' | 'string-list';
  itemFields?: ManualNodeField[];
};

export type ManualNodeProtocol = {
  label: string;
  category: '常用协议' | 'QUIC / VPN' | '特殊出站';
  /** Whether the list view should describe this outbound as a remote endpoint. */
  endpoint: boolean;
  endpointFields?: ManualNodeField[];
  includeCommon?: boolean;
  includeSmux?: boolean;
  fields: ManualNodeField[];
};

const SECTION = {
  server: '服务器',
  auth: '认证',
  protocol: '协议',
  network: '通用网络',
  tls: 'TLS',
  reality: 'Reality',
  ech: 'ECH',
  carrier: 'TLS 载体',
  tlsMirror: 'TLSMirror',
  transport: '传输层',
  smux: 'Sing-Mux',
  plugin: '插件',
  vpn: '隧道',
  dns: '远程 DNS',
  quic: 'QUIC',
  advanced: '高级参数',
} as const;

const choices = (...values: string[]) => values.map(value => ({ value, label: value }));

const shown = (showWhen?: ManualNodeShowWhen) => showWhen ? { showWhen } : {};
const equals = (key: string, value: ManualNodeConditionValue): ManualNodeShowWhen => ({ key, equals: value });
const oneOf = (key: string, ...values: ManualNodeConditionValue[]): ManualNodeShowWhen => ({ key, oneOf: values });
const truthy = (key: string, value = true): ManualNodeShowWhen => ({ key, truthy: value });

export const IP_VERSION_OPTIONS = choices(
  'dual',
  'ipv4',
  'ipv6',
  'ipv4-prefer',
  'ipv6-prefer',
);

export const NETWORK_OPTIONS = choices(
  'tcp',
  'ws',
  'http',
  'h2',
  'grpc',
  'mkcp',
  'mekya',
  'xhttp',
);

export const MANUAL_NODE_ENDPOINT_FIELDS: ManualNodeField[] = [
  { key: 'server', label: '服务器', section: SECTION.server, required: true, placeholder: '域名或 IP 地址' },
  { key: 'port', label: '端口', section: SECTION.server, kind: 'number', required: true, min: 1, max: 65535, step: 1 },
];

export const MANUAL_NODE_COMMON_FIELDS: ManualNodeField[] = [
  {
    key: 'udp',
    label: '允许 UDP',
    section: SECTION.network,
    kind: 'boolean',
    description: '允许 UDP 通过代理；基于 UDP 的协议、Direct 和 DNS 默认开启。',
  },
  { key: 'ip-version', label: '出站 IP 版本', section: SECTION.network, kind: 'select', options: IP_VERSION_OPTIONS },
  { key: 'interface-name', label: '绑定网卡', section: SECTION.network, placeholder: '例如 WLAN / eth0' },
  { key: 'routing-mark', label: '路由标记', section: SECTION.network, kind: 'number' },
  {
    key: 'dialer-proxy',
    label: '前置代理',
    section: SECTION.network,
    placeholder: '节点或策略组名称',
    description: '通过指定代理或策略组建立当前节点的网络连接。',
  },
  {
    key: 'tfo',
    label: 'TCP Fast Open',
    section: SECTION.network,
    kind: 'boolean',
    description: '仅对 TCP 协议生效。',
  },
  {
    key: 'mptcp',
    label: 'TCP MultiPath',
    section: SECTION.network,
    kind: 'boolean',
    description: '仅对 TCP 协议生效。',
  },
];

export const MANUAL_NODE_SMUX_FIELDS: ManualNodeField[] = [
  {
    key: 'smux.enabled',
    label: '启用 Sing-Mux',
    section: SECTION.smux,
    kind: 'boolean',
    description: '仅限使用 TCP 传输的协议。',
  },
  { key: 'smux.protocol', label: '复用协议', section: SECTION.smux, kind: 'select', options: choices('smux', 'yamux', 'h2mux'), ...shown(truthy('smux.enabled')) },
  { key: 'smux.max-connections', label: '最大连接数', section: SECTION.smux, kind: 'number', min: 0, description: '与 max-streams 冲突。', ...shown(truthy('smux.enabled')) },
  { key: 'smux.min-streams', label: '新建连接前最小流数', section: SECTION.smux, kind: 'number', min: 0, description: '与 max-streams 冲突。', ...shown(truthy('smux.enabled')) },
  { key: 'smux.max-streams', label: '新建连接前最大流数', section: SECTION.smux, kind: 'number', min: 0, description: '与 max-connections 和 min-streams 冲突。', ...shown(truthy('smux.enabled')) },
  { key: 'smux.statistic', label: '显示底层连接', section: SECTION.smux, kind: 'boolean', ...shown(truthy('smux.enabled')) },
  { key: 'smux.only-tcp', label: '仅复用 TCP', section: SECTION.smux, kind: 'boolean', ...shown(truthy('smux.enabled')) },
  { key: 'smux.padding', label: '启用填充', section: SECTION.smux, kind: 'boolean', ...shown(truthy('smux.enabled')) },
  { key: 'smux.brutal-opts.enabled', label: '启用 TCP Brutal', section: SECTION.smux, kind: 'boolean', ...shown(truthy('smux.enabled')) },
  { key: 'smux.brutal-opts.up', label: 'Brutal 上行带宽', section: SECTION.smux, kind: 'number', min: 0, description: '单位 Mbps。', ...shown(truthy('smux.brutal-opts.enabled')) },
  { key: 'smux.brutal-opts.down', label: 'Brutal 下行带宽', section: SECTION.smux, kind: 'number', min: 0, description: '单位 Mbps。', ...shown(truthy('smux.brutal-opts.enabled')) },
];

type TlsFieldOptions = {
  serverNameKey?: 'sni' | 'servername';
  toggle?: boolean;
  clientFingerprint?: boolean;
  reality?: boolean;
  carrier?: boolean;
  tlsMirror?: boolean;
};

const CLIENT_FINGERPRINT_OPTIONS = choices(
  'chrome', 'firefox', 'safari', 'ios', 'android', 'edge', '360', 'qq', 'random',
);

function tlsMirrorFields(showWhen?: ManualNodeShowWhen): ManualNodeField[] {
  const active = truthy('tlsmirror-opts.primary-key');
  return [
    {
      key: 'tlsmirror-opts.primary-key',
      label: 'TLSMirror 主密钥',
      section: SECTION.tlsMirror,
      kind: 'password',
      description: '32 字节主密钥的 base64 编码；填写后启用 TLSMirror。',
      ...shown(showWhen),
    },
    { key: 'tlsmirror-opts.explicit-nonce-ciphersuites', label: '显式 Nonce 加密套件', section: SECTION.tlsMirror, kind: 'number-list', ...shown(active) },
    { key: 'tlsmirror-opts.defer-instance-derived-write-time.base-nanoseconds', label: '首次写入基础延迟', section: SECTION.tlsMirror, kind: 'number', min: 0, description: '单位纳秒。', ...shown(active) },
    { key: 'tlsmirror-opts.defer-instance-derived-write-time.uniform-random-multiplier-nanoseconds', label: '首次写入随机延迟乘数', section: SECTION.tlsMirror, kind: 'number', min: 0, description: '单位纳秒。', ...shown(active) },
    { key: 'tlsmirror-opts.transport-layer-padding.enabled', label: '传输层填充', section: SECTION.tlsMirror, kind: 'boolean', ...shown(active) },
    { key: 'tlsmirror-opts.connection-enrolment.primary-ingress-outbound', label: '登记入站控制出站', section: SECTION.tlsMirror, ...shown(active) },
    { key: 'tlsmirror-opts.connection-enrolment.primary-egress-outbound', label: '登记出站控制出站', section: SECTION.tlsMirror, ...shown(active) },
    { key: 'tlsmirror-opts.sequence-watermarking-enabled', label: '序列水印', section: SECTION.tlsMirror, kind: 'boolean', ...shown(active) },
    {
      key: 'tlsmirror-opts.embedded-traffic-generator.steps',
      label: '内嵌 HTTP 载体步骤',
      section: SECTION.tlsMirror,
      kind: 'object-list',
      description: '按顺序执行；next-step 可按权重跳转。',
      ...shown(active),
      itemFields: [
        { key: 'name', label: '步骤名称', section: 'TLSMirror 步骤' },
        { key: 'host', label: '请求主机', section: 'TLSMirror 步骤' },
        { key: 'path', label: '请求路径', section: 'TLSMirror 步骤' },
        { key: 'method', label: '请求方法', section: 'TLSMirror 步骤', placeholder: 'GET' },
        {
          key: 'headers',
          label: '请求头',
          section: 'TLSMirror 步骤',
          kind: 'object-list',
          itemFields: [
            { key: 'name', label: '请求头名称', section: 'TLSMirror 请求头', required: true },
            { key: 'value', label: '单个值', section: 'TLSMirror 请求头' },
            { key: 'values', label: '多个值', section: 'TLSMirror 请求头', kind: 'string-list' },
          ],
        },
        { key: 'connection-ready', label: '步骤后交付连接', section: 'TLSMirror 步骤', kind: 'boolean' },
        { key: 'connection-recall-exit', label: '连接关闭后退出载体', section: 'TLSMirror 步骤', kind: 'boolean' },
        { key: 'h2-do-not-wait-for-download-finish', label: 'H2 不等待响应体完成', section: 'TLSMirror 步骤', kind: 'boolean' },
        { key: 'wait-time.base-nanoseconds', label: '步骤后基础等待', section: 'TLSMirror 步骤', kind: 'number', min: 0 },
        { key: 'wait-time.uniform-random-multiplier-nanoseconds', label: '步骤后随机等待乘数', section: 'TLSMirror 步骤', kind: 'number', min: 0 },
        {
          key: 'next-step',
          label: '下一步骤候选',
          section: 'TLSMirror 步骤',
          kind: 'object-list',
          itemFields: [
            { key: 'weight', label: '权重', section: 'TLSMirror 跳转', kind: 'number', required: true, min: 0 },
            { key: 'goto-location', label: '步骤下标', section: 'TLSMirror 跳转', kind: 'number', required: true, min: 0 },
          ],
        },
      ],
    },
  ];
}

function tlsFields({
  serverNameKey = 'sni',
  toggle = false,
  clientFingerprint = false,
  reality = false,
  carrier = false,
  tlsMirror = false,
}: TlsFieldOptions = {}): ManualNodeField[] {
  const tlsVisible = toggle ? truthy('tls') : undefined;
  const fields: ManualNodeField[] = [];
  if (toggle) fields.push({ key: 'tls', label: '启用 TLS', section: SECTION.tls, kind: 'boolean' });
  fields.push(
    { key: serverNameKey, label: 'SNI / Server Name', section: SECTION.tls, description: '留空时使用 server 地址。', ...shown(tlsVisible) },
    { key: 'fingerprint', label: '证书 SHA-256 指纹', section: SECTION.tls, ...shown(tlsVisible) },
    { key: 'alpn', label: 'ALPN', section: SECTION.tls, kind: 'string-list', placeholder: 'h2\nhttp/1.1', ...shown(tlsVisible) },
    { key: 'skip-cert-verify', label: '跳过证书验证', section: SECTION.tls, kind: 'boolean', ...shown(tlsVisible) },
    { key: 'name-cert-verify', label: '证书 DNSName', section: SECTION.tls, description: '只修改证书 DNSName 校验目标，不修改 SNI。', ...shown(tlsVisible) },
    { key: 'certificate', label: 'mTLS 客户端证书', section: SECTION.tls, kind: 'textarea', description: 'PEM 内容或路径；须与 private-key 同时填写。', ...shown(tlsVisible) },
    { key: 'private-key', label: 'mTLS 客户端私钥', section: SECTION.tls, kind: 'textarea', description: 'PEM 内容或路径；须与 certificate 同时填写。', ...shown(tlsVisible) },
  );
  if (clientFingerprint) {
    fields.push({ key: 'client-fingerprint', label: 'uTLS 客户端指纹', section: SECTION.tls, kind: 'select', options: CLIENT_FINGERPRINT_OPTIONS, ...shown(tlsVisible) });
  }
  fields.push(
    { key: 'ech-opts.enable', label: '启用 ECH', section: SECTION.ech, kind: 'boolean', ...shown(tlsVisible) },
    { key: 'ech-opts.config', label: 'ECH Config', section: SECTION.ech, kind: 'textarea', description: 'base64 编码的 ECH 参数；留空时通过 DNS 解析。', ...shown(truthy('ech-opts.enable')) },
    { key: 'ech-opts.query-server-name', label: 'ECH DNS 查询域名', section: SECTION.ech, ...shown(truthy('ech-opts.enable')) },
  );
  if (reality) {
    fields.push(
      { key: 'reality-opts.public-key', label: 'Reality 公钥', section: SECTION.reality, kind: 'password', description: '填写后启用 Reality。', ...shown(tlsVisible) },
      { key: 'reality-opts.short-id', label: 'Reality Short ID', section: SECTION.reality, ...shown(tlsVisible) },
      { key: 'reality-opts.support-x25519mlkem768', label: '支持 X25519-MLKEM768', section: SECTION.reality, kind: 'boolean', ...shown(tlsVisible) },
    );
  }
  if (carrier) {
    fields.push(
      { key: 'shadow-tls-opts.version', label: 'ShadowTLS 版本', section: SECTION.carrier, kind: 'select', valueType: 'number', options: choices('1', '2', '3'), ...shown(tlsVisible) },
      { key: 'shadow-tls-opts.password', label: 'ShadowTLS 密码', section: SECTION.carrier, kind: 'password', ...shown(tlsVisible) },
      { key: 'restls-opts.password', label: 'Restls 密码', section: SECTION.carrier, kind: 'password', ...shown(tlsVisible) },
      { key: 'restls-opts.version-hint', label: 'Restls TLS 版本提示', section: SECTION.carrier, kind: 'select', options: choices('tls12', 'tls13'), ...shown(tlsVisible) },
      { key: 'restls-opts.restls-script', label: 'Restls 载体脚本', section: SECTION.carrier, kind: 'textarea', ...shown(tlsVisible) },
      { key: 'jls-opts.username', label: 'JLS 用户名', section: SECTION.carrier, ...shown(tlsVisible) },
      { key: 'jls-opts.password', label: 'JLS 密码', section: SECTION.carrier, kind: 'password', ...shown(tlsVisible) },
    );
  }
  if (tlsMirror) fields.push(...tlsMirrorFields(tlsVisible));
  return fields;
}

function kcpFields(prefix: string, showWhen: ManualNodeShowWhen): ManualNodeField[] {
  return [
    { key: `${prefix}.mtu`, label: 'KCP MTU', section: SECTION.transport, kind: 'number', min: 0, ...shown(showWhen) },
    { key: `${prefix}.tti`, label: 'KCP TTI', section: SECTION.transport, kind: 'number', min: 0, description: '传输时间间隔，单位毫秒。', ...shown(showWhen) },
    { key: `${prefix}.uplink-capacity`, label: 'KCP 上行容量', section: SECTION.transport, kind: 'number', min: 0, description: '单位 MB/s。', ...shown(showWhen) },
    { key: `${prefix}.downlink-capacity`, label: 'KCP 下行容量', section: SECTION.transport, kind: 'number', min: 0, description: '单位 MB/s。', ...shown(showWhen) },
    { key: `${prefix}.congestion`, label: 'KCP 拥塞控制', section: SECTION.transport, kind: 'boolean', ...shown(showWhen) },
    { key: `${prefix}.write-buffer`, label: 'KCP 写缓冲区', section: SECTION.transport, kind: 'number', min: 0, description: '单位字节。', ...shown(showWhen) },
    { key: `${prefix}.read-buffer`, label: 'KCP 读缓冲区', section: SECTION.transport, kind: 'number', min: 0, description: '单位字节。', ...shown(showWhen) },
    { key: `${prefix}.seed`, label: 'KCP AES-GCM 种子', section: SECTION.transport, ...shown(showWhen) },
    { key: `${prefix}.header`, label: 'KCP 伪装包头', section: SECTION.transport, kind: 'select', options: choices('none', 'srtp', 'utp', 'wechat-video', 'dtls', 'wireguard'), ...shown(showWhen) },
  ];
}

function xmuxFields(prefix: string, showWhen: ManualNodeShowWhen): ManualNodeField[] {
  return [
    { key: `${prefix}.max-concurrency`, label: 'XMUX 最大并发', section: SECTION.transport, placeholder: '16-32', description: '可填写范围；与 max-connections 冲突。', ...shown(showWhen) },
    { key: `${prefix}.max-connections`, label: 'XMUX 最大连接数', section: SECTION.transport, placeholder: '0', description: '可填写范围；与 max-concurrency 冲突。', ...shown(showWhen) },
    { key: `${prefix}.c-max-reuse-times`, label: '连接最大复用次数', section: SECTION.transport, placeholder: '0', ...shown(showWhen) },
    { key: `${prefix}.h-max-request-times`, label: '连接累计请求上限', section: SECTION.transport, placeholder: '600-900', ...shown(showWhen) },
    { key: `${prefix}.h-max-reusable-secs`, label: '连接最大可复用秒数', section: SECTION.transport, placeholder: '1800-3000', ...shown(showWhen) },
    { key: `${prefix}.h-keep-alive-period`, label: 'H2/H3 保活周期', section: SECTION.transport, kind: 'number', description: '单位秒；允许负数，-1 可关闭空闲保活。', ...shown(showWhen) },
  ];
}

function xhttpFields(): ManualNodeField[] {
  const active = equals('network', 'xhttp');
  const downloadTls = truthy('xhttp-opts.download-settings.tls');
  return [
    { key: 'xhttp-opts.path', label: 'XHTTP 路径', section: SECTION.transport, ...shown(active) },
    { key: 'xhttp-opts.host', label: 'XHTTP Host', section: SECTION.transport, ...shown(active) },
    { key: 'xhttp-opts.mode', label: 'XHTTP 模式', section: SECTION.transport, kind: 'select', options: choices('auto', 'stream-one', 'stream-up', 'packet-up'), ...shown(active) },
    { key: 'xhttp-opts.headers', label: 'XHTTP 请求头', section: SECTION.transport, kind: 'key-value-list', valueKind: 'text', ...shown(active) },
    { key: 'xhttp-opts.no-grpc-header', label: '不发送 gRPC 伪装头', section: SECTION.transport, kind: 'boolean', ...shown(active) },
    { key: 'xhttp-opts.x-padding-bytes', label: '填充长度范围', section: SECTION.transport, placeholder: '100-1000', ...shown(active) },
    { key: 'xhttp-opts.x-padding-obfs-mode', label: '填充混淆', section: SECTION.transport, kind: 'boolean', ...shown(active) },
    { key: 'xhttp-opts.x-padding-key', label: '填充键名', section: SECTION.transport, ...shown(active) },
    { key: 'xhttp-opts.x-padding-header', label: '填充请求头名', section: SECTION.transport, ...shown(active) },
    { key: 'xhttp-opts.x-padding-placement', label: '填充位置', section: SECTION.transport, kind: 'select', options: choices('queryInHeader', 'cookie', 'header', 'query'), ...shown(active) },
    { key: 'xhttp-opts.x-padding-method', label: '填充生成方式', section: SECTION.transport, kind: 'select', options: choices('repeat-x', 'tokenish'), ...shown(active) },
    { key: 'xhttp-opts.uplink-http-method', label: '上行 HTTP 方法', section: SECTION.transport, kind: 'select', options: choices('POST', 'PUT', 'PATCH', 'DELETE'), ...shown(active) },
    { key: 'xhttp-opts.session-placement', label: '会话 ID 位置', section: SECTION.transport, kind: 'select', options: choices('path', 'query', 'cookie', 'header'), ...shown(active) },
    { key: 'xhttp-opts.session-key', label: '会话 ID 键名', section: SECTION.transport, ...shown(active) },
    { key: 'xhttp-opts.session-table', label: '会话 ID 字符表', section: SECTION.transport, kind: 'select', options: choices('uuid', 'ALPHABET', 'Alphabet', 'BASE36', 'Base62', 'HEX', 'alphabet', 'base36', 'hex', 'number'), ...shown(active) },
    { key: 'xhttp-opts.session-length', label: '会话 ID 长度范围', section: SECTION.transport, placeholder: '16-32', ...shown(active) },
    { key: 'xhttp-opts.seq-placement', label: '序列号位置', section: SECTION.transport, kind: 'select', options: choices('path', 'query', 'cookie', 'header'), ...shown(active) },
    { key: 'xhttp-opts.seq-key', label: '序列号键名', section: SECTION.transport, ...shown(active) },
    { key: 'xhttp-opts.uplink-data-placement', label: '上行数据位置', section: SECTION.transport, kind: 'select', options: choices('body', 'cookie', 'header'), ...shown(active) },
    { key: 'xhttp-opts.uplink-data-key', label: '上行数据键名', section: SECTION.transport, ...shown(active) },
    { key: 'xhttp-opts.uplink-chunk-size', label: '上行分块大小', section: SECTION.transport, kind: 'number', min: 0, description: '非 body 位置时适用；0 表示自动，非零值至少 64 字节。', ...shown(active) },
    { key: 'xhttp-opts.sc-max-each-post-bytes', label: '单次 POST 最大字节数', section: SECTION.transport, kind: 'number', min: 0, ...shown(active) },
    { key: 'xhttp-opts.sc-min-posts-interval-ms', label: 'POST 最小间隔', section: SECTION.transport, kind: 'number', min: 0, description: '单位毫秒。', ...shown(active) },
    ...xmuxFields('xhttp-opts.reuse-settings', active),
    { key: 'xhttp-opts.download-settings.path', label: '下行 XHTTP 路径', section: SECTION.transport, ...shown(active) },
    { key: 'xhttp-opts.download-settings.host', label: '下行 XHTTP Host', section: SECTION.transport, ...shown(active) },
    { key: 'xhttp-opts.download-settings.headers', label: '下行请求头', section: SECTION.transport, kind: 'key-value-list', valueKind: 'text', ...shown(active) },
    ...xmuxFields('xhttp-opts.download-settings.reuse-settings', active),
    { key: 'xhttp-opts.download-settings.server', label: '下行代理服务器', section: SECTION.transport, ...shown(active) },
    { key: 'xhttp-opts.download-settings.port', label: '下行代理端口', section: SECTION.transport, kind: 'number', min: 1, max: 65535, ...shown(active) },
    { key: 'xhttp-opts.download-settings.tls', label: '下行代理 TLS', section: SECTION.transport, kind: 'boolean', ...shown(active) },
    { key: 'xhttp-opts.download-settings.servername', label: '下行 Server Name', section: SECTION.transport, ...shown(downloadTls) },
    { key: 'xhttp-opts.download-settings.alpn', label: '下行 ALPN', section: SECTION.transport, kind: 'string-list', ...shown(downloadTls) },
    { key: 'xhttp-opts.download-settings.skip-cert-verify', label: '下行跳过证书验证', section: SECTION.transport, kind: 'boolean', ...shown(downloadTls) },
    { key: 'xhttp-opts.download-settings.name-cert-verify', label: '下行证书 DNSName', section: SECTION.transport, ...shown(downloadTls) },
    { key: 'xhttp-opts.download-settings.fingerprint', label: '下行证书指纹', section: SECTION.transport, ...shown(downloadTls) },
    { key: 'xhttp-opts.download-settings.certificate', label: '下行 mTLS 证书', section: SECTION.transport, kind: 'textarea', ...shown(downloadTls) },
    { key: 'xhttp-opts.download-settings.private-key', label: '下行 mTLS 私钥', section: SECTION.transport, kind: 'textarea', ...shown(downloadTls) },
    { key: 'xhttp-opts.download-settings.client-fingerprint', label: '下行 uTLS 指纹', section: SECTION.transport, kind: 'select', options: CLIENT_FINGERPRINT_OPTIONS, ...shown(downloadTls) },
    { key: 'xhttp-opts.download-settings.ech-opts.enable', label: '下行启用 ECH', section: SECTION.transport, kind: 'boolean', ...shown(downloadTls) },
    { key: 'xhttp-opts.download-settings.ech-opts.config', label: '下行 ECH Config', section: SECTION.transport, kind: 'textarea', ...shown(truthy('xhttp-opts.download-settings.ech-opts.enable')) },
    { key: 'xhttp-opts.download-settings.ech-opts.query-server-name', label: '下行 ECH 查询域名', section: SECTION.transport, ...shown(truthy('xhttp-opts.download-settings.ech-opts.enable')) },
    { key: 'xhttp-opts.download-settings.reality-opts.public-key', label: '下行 Reality 公钥', section: SECTION.transport, kind: 'password', ...shown(downloadTls) },
    { key: 'xhttp-opts.download-settings.reality-opts.short-id', label: '下行 Reality Short ID', section: SECTION.transport, ...shown(downloadTls) },
    { key: 'xhttp-opts.download-settings.reality-opts.support-x25519mlkem768', label: '下行 Reality ML-KEM', section: SECTION.transport, kind: 'boolean', ...shown(downloadTls) },
    { key: 'xhttp-opts.download-settings.shadow-tls-opts.version', label: '下行 ShadowTLS 版本', section: SECTION.transport, kind: 'select', valueType: 'number', options: choices('1', '2', '3'), ...shown(downloadTls) },
    { key: 'xhttp-opts.download-settings.shadow-tls-opts.password', label: '下行 ShadowTLS 密码', section: SECTION.transport, kind: 'password', ...shown(downloadTls) },
    { key: 'xhttp-opts.download-settings.restls-opts.password', label: '下行 Restls 密码', section: SECTION.transport, kind: 'password', ...shown(downloadTls) },
    { key: 'xhttp-opts.download-settings.restls-opts.version-hint', label: '下行 Restls 版本提示', section: SECTION.transport, kind: 'select', options: choices('tls12', 'tls13'), ...shown(downloadTls) },
    { key: 'xhttp-opts.download-settings.restls-opts.restls-script', label: '下行 Restls 脚本', section: SECTION.transport, kind: 'textarea', ...shown(downloadTls) },
    { key: 'xhttp-opts.download-settings.jls-opts.username', label: '下行 JLS 用户名', section: SECTION.transport, ...shown(downloadTls) },
    { key: 'xhttp-opts.download-settings.jls-opts.password', label: '下行 JLS 密码', section: SECTION.transport, kind: 'password', ...shown(downloadTls) },
  ];
}

function transportFields(networks: string[]): ManualNodeField[] {
  const fields: ManualNodeField[] = [
    { key: 'network', label: '传输层', section: SECTION.transport, kind: 'select', options: choices(...networks) },
  ];
  if (networks.includes('http')) fields.push(
    { key: 'http-opts.method', label: 'HTTP 方法', section: SECTION.transport, ...shown(equals('network', 'http')) },
    { key: 'http-opts.path', label: 'HTTP 路径', section: SECTION.transport, kind: 'string-list', ...shown(equals('network', 'http')) },
    { key: 'http-opts.headers', label: 'HTTP 请求头', section: SECTION.transport, kind: 'key-value-list', valueKind: 'string-list', ...shown(equals('network', 'http')) },
  );
  if (networks.includes('h2')) fields.push(
    { key: 'h2-opts.host', label: 'HTTP/2 Host', section: SECTION.transport, kind: 'string-list', ...shown(equals('network', 'h2')) },
    { key: 'h2-opts.path', label: 'HTTP/2 路径', section: SECTION.transport, ...shown(equals('network', 'h2')) },
  );
  if (networks.includes('grpc')) fields.push(
    { key: 'grpc-opts.grpc-service-name', label: 'gRPC 服务名', section: SECTION.transport, ...shown(equals('network', 'grpc')) },
    { key: 'grpc-opts.grpc-user-agent', label: 'gRPC User-Agent', section: SECTION.transport, ...shown(equals('network', 'grpc')) },
    { key: 'grpc-opts.ping-interval', label: 'gRPC 心跳间隔', section: SECTION.transport, kind: 'number', min: 0, description: '单位秒，0 表示关闭。', ...shown(equals('network', 'grpc')) },
    { key: 'grpc-opts.max-connections', label: 'gRPC 最大连接数', section: SECTION.transport, kind: 'number', min: 0, description: '与 max-streams 冲突。', ...shown(equals('network', 'grpc')) },
    { key: 'grpc-opts.min-streams', label: 'gRPC 最小流数', section: SECTION.transport, kind: 'number', min: 0, description: '与 max-streams 冲突。', ...shown(equals('network', 'grpc')) },
    { key: 'grpc-opts.max-streams', label: 'gRPC 最大流数', section: SECTION.transport, kind: 'number', min: 0, description: '与 max-connections 和 min-streams 冲突。', ...shown(equals('network', 'grpc')) },
  );
  if (networks.includes('ws')) fields.push(
    { key: 'ws-opts.path', label: 'WebSocket 路径', section: SECTION.transport, ...shown(equals('network', 'ws')) },
    { key: 'ws-opts.headers', label: 'WebSocket 请求头', section: SECTION.transport, kind: 'key-value-list', valueKind: 'text', ...shown(equals('network', 'ws')) },
    { key: 'ws-opts.max-early-data', label: 'WebSocket Early Data 阈值', section: SECTION.transport, kind: 'number', min: 0, ...shown(equals('network', 'ws')) },
    { key: 'ws-opts.early-data-header-name', label: 'Early Data 请求头名', section: SECTION.transport, ...shown(equals('network', 'ws')) },
    { key: 'ws-opts.v2ray-http-upgrade', label: '使用 HTTP Upgrade', section: SECTION.transport, kind: 'boolean', ...shown(equals('network', 'ws')) },
    { key: 'ws-opts.v2ray-http-upgrade-fast-open', label: 'HTTP Upgrade Fast Open', section: SECTION.transport, kind: 'boolean', ...shown(equals('network', 'ws')) },
  );
  if (networks.includes('mkcp')) fields.push(...kcpFields('mkcp-opts', equals('network', 'mkcp')));
  if (networks.includes('mekya')) fields.push(
    { key: 'mekya-opts.url', label: 'Mekya 服务端 URL', section: SECTION.transport, ...shown(equals('network', 'mekya')) },
    { key: 'mekya-opts.max-write-delay', label: 'Mekya 最大写等待', section: SECTION.transport, kind: 'number', min: 0, description: '单位毫秒。', ...shown(equals('network', 'mekya')) },
    { key: 'mekya-opts.max-request-size', label: 'Mekya 最大请求负载', section: SECTION.transport, kind: 'number', min: 0, description: '单位字节。', ...shown(equals('network', 'mekya')) },
    { key: 'mekya-opts.polling-interval-initial', label: 'Mekya 初始轮询间隔', section: SECTION.transport, kind: 'number', min: 0, description: '单位毫秒。', ...shown(equals('network', 'mekya')) },
    { key: 'mekya-opts.h2-pool-size', label: 'Mekya HTTP/2 连接池', section: SECTION.transport, kind: 'number', min: 0, ...shown(equals('network', 'mekya')) },
    ...kcpFields('mekya-opts.kcp', equals('network', 'mekya')),
  );
  if (networks.includes('xhttp')) fields.push(...xhttpFields());
  return fields;
}

const COMMON_CIPHER_OPTIONS = choices(
  'AES-128-GCM', 'AES-256-GCM', 'AES-128-CBC', 'AES-256-CBC', 'AES-CBC', 'CHACHA20-POLY1305',
);

export const MANUAL_NODE_PROTOCOLS: Record<string, ManualNodeProtocol> = {
  ss: {
    label: 'Shadowsocks',
    category: '常用协议',
    endpoint: true,
    fields: [
      {
        key: 'cipher',
        label: '加密方式',
        section: SECTION.protocol,
        kind: 'select',
        required: true,
        options: choices(
          'aes-128-ctr', 'aes-192-ctr', 'aes-256-ctr',
          'aes-128-cfb', 'aes-192-cfb', 'aes-256-cfb',
          'aes-128-gcm', 'aes-192-gcm', 'aes-256-gcm',
          'aes-128-ccm', 'aes-192-ccm', 'aes-256-ccm',
          'aes-128-gcm-siv', 'aes-256-gcm-siv',
          'chacha20-ietf', 'chacha20', 'xchacha20',
          'chacha20-ietf-poly1305', 'xchacha20-ietf-poly1305',
          'chacha8-ietf-poly1305', 'xchacha8-ietf-poly1305',
          '2022-blake3-aes-128-gcm', '2022-blake3-aes-256-gcm',
          '2022-blake3-chacha20-poly1305',
          'lea-128-gcm', 'lea-192-gcm', 'lea-256-gcm',
          'rabbit128-poly1305', 'aegis-128l', 'aegis-256', 'aez-384',
          'deoxys-ii-256-128', 'rc4-md5', 'none',
        ),
      },
      { key: 'password', label: '密码', section: SECTION.auth, kind: 'password', required: true },
      { key: 'udp-over-tcp', label: 'UDP over TCP', section: SECTION.protocol, kind: 'boolean' },
      { key: 'udp-over-tcp-version', label: 'UDP over TCP 版本', section: SECTION.protocol, kind: 'select', valueType: 'number', options: choices('1', '2'), ...shown(truthy('udp-over-tcp')) },
      { key: 'plugin', label: '插件', section: SECTION.plugin, kind: 'select', options: choices('obfs', 'v2ray-plugin', 'gost-plugin', 'shadow-tls', 'restls', 'kcptun', 'jls') },
      {
        key: 'client-fingerprint',
        label: '插件 uTLS 指纹',
        section: SECTION.plugin,
        kind: 'select',
        options: CLIENT_FINGERPRINT_OPTIONS,
        ...shown(oneOf('plugin', 'shadow-tls', 'restls', 'jls')),
      },
      {
        key: 'plugin-opts.mode',
        label: '插件模式 / KCP Profile',
        section: SECTION.plugin,
        description: 'obfs 使用 http/tls；v2ray-plugin 与 gost-plugin 使用 websocket；kcptun 使用 fast3/fast2/fast/normal/manual。',
        ...shown(oneOf('plugin', 'obfs', 'v2ray-plugin', 'gost-plugin', 'kcptun')),
      },
      { key: 'plugin-opts.host', label: '插件 Host', section: SECTION.plugin, ...shown(oneOf('plugin', 'obfs', 'v2ray-plugin', 'gost-plugin', 'shadow-tls', 'restls', 'jls')) },
      { key: 'plugin-opts.tls', label: '插件 TLS', section: SECTION.plugin, kind: 'boolean', ...shown(oneOf('plugin', 'v2ray-plugin', 'gost-plugin')) },
      { key: 'plugin-opts.fingerprint', label: '插件证书指纹', section: SECTION.plugin, ...shown(oneOf('plugin', 'v2ray-plugin', 'gost-plugin')) },
      { key: 'plugin-opts.skip-cert-verify', label: '插件跳过证书验证', section: SECTION.plugin, kind: 'boolean', ...shown(oneOf('plugin', 'v2ray-plugin', 'gost-plugin')) },
      { key: 'plugin-opts.name-cert-verify', label: '插件证书 DNSName', section: SECTION.plugin, ...shown(oneOf('plugin', 'v2ray-plugin', 'gost-plugin')) },
      { key: 'plugin-opts.path', label: '插件 WebSocket 路径', section: SECTION.plugin, ...shown(oneOf('plugin', 'v2ray-plugin', 'gost-plugin')) },
      { key: 'plugin-opts.mux', label: '插件多路复用', section: SECTION.plugin, kind: 'boolean', ...shown(oneOf('plugin', 'v2ray-plugin', 'gost-plugin')) },
      { key: 'plugin-opts.headers', label: '插件请求头', section: SECTION.plugin, kind: 'key-value-list', valueKind: 'text', ...shown(oneOf('plugin', 'v2ray-plugin', 'gost-plugin')) },
      { key: 'plugin-opts.v2ray-http-upgrade', label: '插件 HTTP Upgrade', section: SECTION.plugin, kind: 'boolean', ...shown(equals('plugin', 'v2ray-plugin')) },
      { key: 'plugin-opts.password', label: '插件密码', section: SECTION.plugin, kind: 'password', ...shown(oneOf('plugin', 'shadow-tls', 'restls', 'jls')) },
      { key: 'plugin-opts.version', label: 'ShadowTLS 版本', section: SECTION.plugin, kind: 'select', valueType: 'number', options: choices('1', '2', '3'), ...shown(equals('plugin', 'shadow-tls')) },
      { key: 'plugin-opts.alpn', label: 'JLS ALPN', section: SECTION.plugin, kind: 'string-list', ...shown(equals('plugin', 'jls')) },
      { key: 'plugin-opts.version-hint', label: 'Restls TLS 版本提示', section: SECTION.plugin, kind: 'select', options: choices('tls12', 'tls13'), ...shown(equals('plugin', 'restls')) },
      { key: 'plugin-opts.restls-script', label: 'Restls 载体脚本', section: SECTION.plugin, kind: 'textarea', ...shown(equals('plugin', 'restls')) },
      { key: 'plugin-opts.username', label: 'JLS 用户名', section: SECTION.plugin, ...shown(equals('plugin', 'jls')) },
      { key: 'plugin-opts.key', label: 'Kcptun 预共享密钥', section: SECTION.plugin, kind: 'password', ...shown(equals('plugin', 'kcptun')) },
      { key: 'plugin-opts.crypt', label: 'Kcptun 加密', section: SECTION.plugin, kind: 'select', options: choices('aes', 'aes-128', 'aes-128-gcm', 'aes-192', 'salsa20', 'blowfish', 'twofish', 'cast5', '3des', 'tea', 'xtea', 'xor', 'none', 'null'), ...shown(equals('plugin', 'kcptun')) },
      { key: 'plugin-opts.conn', label: 'Kcptun UDP 连接数', section: SECTION.plugin, kind: 'number', min: 0, ...shown(equals('plugin', 'kcptun')) },
      { key: 'plugin-opts.autoexpire', label: 'Kcptun 自动过期', section: SECTION.plugin, kind: 'number', min: 0, description: '单位秒，0 表示关闭。', ...shown(equals('plugin', 'kcptun')) },
      { key: 'plugin-opts.scavengettl', label: 'Kcptun 过期连接 TTL', section: SECTION.plugin, kind: 'number', min: 0, description: '单位秒。', ...shown(equals('plugin', 'kcptun')) },
      { key: 'plugin-opts.mtu', label: 'Kcptun MTU', section: SECTION.plugin, kind: 'number', min: 0, ...shown(equals('plugin', 'kcptun')) },
      { key: 'plugin-opts.ratelimit', label: 'Kcptun 单连接限速', section: SECTION.plugin, kind: 'number', min: 0, description: '单位 bytes/s，0 表示关闭。', ...shown(equals('plugin', 'kcptun')) },
      { key: 'plugin-opts.sndwnd', label: 'Kcptun 发送窗口', section: SECTION.plugin, kind: 'number', min: 0, ...shown(equals('plugin', 'kcptun')) },
      { key: 'plugin-opts.rcvwnd', label: 'Kcptun 接收窗口', section: SECTION.plugin, kind: 'number', min: 0, ...shown(equals('plugin', 'kcptun')) },
      { key: 'plugin-opts.datashard', label: 'Kcptun Data Shard', section: SECTION.plugin, kind: 'number', min: 0, ...shown(equals('plugin', 'kcptun')) },
      { key: 'plugin-opts.parityshard', label: 'Kcptun Parity Shard', section: SECTION.plugin, kind: 'number', min: 0, ...shown(equals('plugin', 'kcptun')) },
      { key: 'plugin-opts.dscp', label: 'Kcptun DSCP', section: SECTION.plugin, kind: 'number', min: 0, max: 63, ...shown(equals('plugin', 'kcptun')) },
      { key: 'plugin-opts.nocomp', label: 'Kcptun 禁用压缩', section: SECTION.plugin, kind: 'boolean', ...shown(equals('plugin', 'kcptun')) },
      { key: 'plugin-opts.acknodelay', label: 'Kcptun 立即 ACK', section: SECTION.plugin, kind: 'boolean', ...shown(equals('plugin', 'kcptun')) },
      { key: 'plugin-opts.nodelay', label: 'Kcptun No Delay', section: SECTION.plugin, kind: 'number', ...shown(equals('plugin', 'kcptun')) },
      { key: 'plugin-opts.interval', label: 'Kcptun Interval', section: SECTION.plugin, kind: 'number', min: 0, ...shown(equals('plugin', 'kcptun')) },
      { key: 'plugin-opts.resend', label: 'Kcptun Resend', section: SECTION.plugin, kind: 'number', min: 0, ...shown(equals('plugin', 'kcptun')) },
      { key: 'plugin-opts.sockbuf', label: 'Kcptun Socket Buffer', section: SECTION.plugin, kind: 'number', min: 0, ...shown(equals('plugin', 'kcptun')) },
      { key: 'plugin-opts.smuxver', label: 'Kcptun SMux 版本', section: SECTION.plugin, kind: 'select', valueType: 'number', options: choices('1', '2'), ...shown(equals('plugin', 'kcptun')) },
      { key: 'plugin-opts.smuxbuf', label: 'Kcptun SMux Buffer', section: SECTION.plugin, kind: 'number', min: 0, ...shown(equals('plugin', 'kcptun')) },
      { key: 'plugin-opts.framesize', label: 'Kcptun Frame Size', section: SECTION.plugin, kind: 'number', min: 0, ...shown(equals('plugin', 'kcptun')) },
      { key: 'plugin-opts.streambuf', label: 'Kcptun Stream Buffer', section: SECTION.plugin, kind: 'number', min: 0, ...shown(equals('plugin', 'kcptun')) },
      { key: 'plugin-opts.keepalive', label: 'Kcptun 心跳间隔', section: SECTION.plugin, kind: 'number', min: 0, description: '单位秒。', ...shown(equals('plugin', 'kcptun')) },
    ],
  },
  ssr: {
    label: 'ShadowsocksR',
    category: '常用协议',
    endpoint: true,
    fields: [
      { key: 'cipher', label: '加密方式', section: SECTION.protocol, required: true, placeholder: 'chacha20-ietf' },
      { key: 'password', label: '密码', section: SECTION.auth, kind: 'password', required: true },
      { key: 'protocol', label: '协议', section: SECTION.protocol, required: true, placeholder: 'auth_sha1_v4' },
      { key: 'protocol-param', label: '协议参数', section: SECTION.protocol },
      { key: 'obfs', label: '混淆', section: SECTION.protocol, required: true, placeholder: 'tls1.2_ticket_auth' },
      { key: 'obfs-param', label: '混淆参数', section: SECTION.protocol },
    ],
  },
  vmess: {
    label: 'VMess',
    category: '常用协议',
    endpoint: true,
    fields: [
      { key: 'uuid', label: 'UUID', section: SECTION.auth, required: true },
      { key: 'alterId', label: 'Alter ID', section: SECTION.protocol, kind: 'number', required: true, min: 0, description: '非 0 时启用旧协议。' },
      { key: 'cipher', label: '加密方式', section: SECTION.protocol, kind: 'select', required: true, options: choices('auto', 'none', 'zero', 'aes-128-gcm', 'chacha20-poly1305') },
      { key: 'packet-encoding', label: 'UDP 包编码', section: SECTION.protocol, kind: 'select', options: choices('packetaddr', 'xudp') },
      { key: 'global-padding', label: '全局填充', section: SECTION.protocol, kind: 'boolean' },
      { key: 'authenticated-length', label: '认证长度块', section: SECTION.protocol, kind: 'boolean' },
      ...tlsFields({ serverNameKey: 'servername', toggle: true, clientFingerprint: true, reality: true, carrier: true, tlsMirror: true }),
      ...transportFields(['tcp', 'ws', 'http', 'h2', 'grpc', 'mkcp', 'mekya']),
    ],
  },
  vless: {
    label: 'VLESS',
    category: '常用协议',
    endpoint: true,
    fields: [
      { key: 'uuid', label: 'UUID', section: SECTION.auth, required: true },
      { key: 'flow', label: 'Flow', section: SECTION.protocol, kind: 'select', options: choices('xtls-rprx-vision') },
      { key: 'packet-encoding', label: 'UDP 包编码', section: SECTION.protocol, kind: 'select', options: choices('packetaddr', 'xudp') },
      { key: 'encryption', label: 'VLESS Encryption', section: SECTION.protocol, kind: 'textarea', description: 'VLESS Encryption 客户端配置串；留空使用普通 VLESS。' },
      ...tlsFields({ serverNameKey: 'servername', toggle: true, clientFingerprint: true, reality: true, carrier: true }),
      ...transportFields(['tcp', 'ws', 'http', 'h2', 'grpc', 'xhttp']),
    ],
  },
  trojan: {
    label: 'Trojan',
    category: '常用协议',
    endpoint: true,
    fields: [
      { key: 'password', label: '密码', section: SECTION.auth, kind: 'password', required: true },
      { key: 'ss-opts.enabled', label: 'Trojan-Go Shadowsocks AEAD', section: SECTION.protocol, kind: 'boolean' },
      { key: 'ss-opts.method', label: 'Shadowsocks AEAD 加密', section: SECTION.protocol, kind: 'select', options: choices('aes-128-gcm', 'aes-256-gcm', 'chacha20-ietf-poly1305'), ...shown(truthy('ss-opts.enabled')) },
      { key: 'ss-opts.password', label: 'Shadowsocks AEAD 密码', section: SECTION.protocol, kind: 'password', ...shown(truthy('ss-opts.enabled')) },
      ...tlsFields({ clientFingerprint: true, reality: true, carrier: true }),
      ...transportFields(['tcp', 'ws', 'grpc']),
    ],
  },
  http: {
    label: 'HTTP / HTTPS',
    category: '常用协议',
    endpoint: true,
    fields: [
      { key: 'username', label: '用户名', section: SECTION.auth },
      { key: 'password', label: '密码', section: SECTION.auth, kind: 'password' },
      { key: 'headers', label: '代理请求头', section: SECTION.protocol, kind: 'key-value-list', valueKind: 'text' },
      ...tlsFields({ toggle: true, carrier: true }),
    ],
  },
  socks5: {
    label: 'SOCKS5',
    category: '常用协议',
    endpoint: true,
    fields: [
      { key: 'username', label: '用户名', section: SECTION.auth },
      { key: 'password', label: '密码', section: SECTION.auth, kind: 'password' },
      ...tlsFields({ toggle: true, carrier: true }),
    ],
  },
  snell: {
    label: 'Snell',
    category: '常用协议',
    endpoint: true,
    fields: [
      { key: 'psk', label: 'PSK', section: SECTION.auth, kind: 'password', required: true },
      { key: 'version', label: '版本', section: SECTION.protocol, kind: 'select', valueType: 'number', options: choices('1', '2', '3', '4', '5'), description: 'v3/v4/v5 支持 UDP；v4/v5 支持 reuse。' },
      { key: 'reuse', label: '连接复用', section: SECTION.protocol, kind: 'boolean', description: '仅 v4/v5 支持。' },
      { key: 'client-fingerprint', label: '客户端指纹', section: SECTION.protocol, kind: 'select', options: CLIENT_FINGERPRINT_OPTIONS },
      { key: 'obfs-opts.mode', label: '混淆模式', section: SECTION.protocol, kind: 'select', options: choices('http', 'tls', 'shadow-tls', 'restls', 'jls') },
      { key: 'obfs-opts.host', label: '混淆 Host', section: SECTION.protocol, ...shown(truthy('obfs-opts.mode')) },
      { key: 'obfs-opts.password', label: '混淆密码', section: SECTION.protocol, kind: 'password', ...shown(oneOf('obfs-opts.mode', 'shadow-tls', 'restls', 'jls')) },
      { key: 'obfs-opts.version', label: 'ShadowTLS 版本', section: SECTION.protocol, kind: 'select', valueType: 'number', options: choices('1', '2', '3'), ...shown(equals('obfs-opts.mode', 'shadow-tls')) },
      { key: 'obfs-opts.alpn', label: 'ShadowTLS ALPN', section: SECTION.protocol, kind: 'string-list', ...shown(equals('obfs-opts.mode', 'shadow-tls')) },
      { key: 'obfs-opts.username', label: 'JLS 用户名', section: SECTION.protocol, ...shown(equals('obfs-opts.mode', 'jls')) },
      { key: 'obfs-opts.version-hint', label: 'Restls TLS 版本提示', section: SECTION.protocol, kind: 'select', options: choices('tls12', 'tls13'), ...shown(equals('obfs-opts.mode', 'restls')) },
      { key: 'obfs-opts.restls-script', label: 'Restls 载体脚本', section: SECTION.protocol, kind: 'textarea', ...shown(equals('obfs-opts.mode', 'restls')) },
    ],
  },
  ssh: {
    label: 'SSH',
    category: '常用协议',
    endpoint: true,
    fields: [
      { key: 'username', label: '用户名', section: SECTION.auth, required: true },
      { key: 'password', label: '密码', section: SECTION.auth, kind: 'password' },
      { key: 'private-key', label: '私钥内容或路径', section: SECTION.auth, kind: 'textarea' },
      { key: 'private-key-passphrase', label: '私钥口令', section: SECTION.auth, kind: 'password' },
      { key: 'host-key', label: 'Host Key', section: SECTION.auth, kind: 'string-list', description: '留空接受所有主机密钥。' },
      { key: 'host-key-algorithms', label: 'Host Key Algorithms', section: SECTION.auth, kind: 'string-list' },
    ],
  },
  hysteria: {
    label: 'Hysteria',
    category: 'QUIC / VPN',
    endpoint: true,
    includeSmux: false,
    fields: [
      { key: 'auth-str', label: '认证字符串', section: SECTION.auth, kind: 'password', required: true },
      { key: 'ports', label: '端口跳跃范围', section: SECTION.server, placeholder: '1000,2000-3000,4000', description: '启用端口跳跃时 port 仍不可省略。' },
      { key: 'protocol', label: '协议伪装', section: SECTION.protocol, kind: 'select', options: choices('udp', 'wechat-video', 'faketcp') },
      { key: 'up', label: '上行带宽', section: SECTION.quic, placeholder: '30 Mbps', description: '不写单位时默认为 Mbps。' },
      { key: 'down', label: '下行带宽', section: SECTION.quic, placeholder: '200 Mbps', description: '不写单位时默认为 Mbps。' },
      { key: 'obfs', label: '混淆字符串', section: SECTION.protocol },
      ...tlsFields(),
      { key: 'recv-window-conn', label: '单连接接收窗口', section: SECTION.quic, kind: 'number', min: 0 },
      { key: 'recv-window', label: '连接接收窗口', section: SECTION.quic, kind: 'number', min: 0 },
      { key: 'disable_mtu_discovery', label: '禁用 MTU 探测', section: SECTION.quic, kind: 'boolean' },
      { key: 'fast-open', label: 'Fast Open', section: SECTION.quic, kind: 'boolean' },
    ],
  },
  hysteria2: {
    label: 'Hysteria2',
    category: 'QUIC / VPN',
    endpoint: true,
    includeSmux: false,
    fields: [
      { key: 'password', label: '认证密码', section: SECTION.auth, kind: 'password', required: true },
      { key: 'ports', label: '端口跳跃范围', section: SECTION.server, placeholder: '443-8443', description: '填写后启用端口跳跃并忽略 port。' },
      { key: 'hop-interval', label: '端口跳跃间隔', section: SECTION.server, placeholder: '30 或 15-30', description: '单位秒；仅支持单个数值或单个范围。' },
      { key: 'up', label: 'Brutal 上行带宽', section: SECTION.quic, placeholder: '30 Mbps', description: '不写单位时默认为 Mbps。' },
      { key: 'down', label: 'Brutal 下行带宽', section: SECTION.quic, placeholder: '200 Mbps', description: '不写单位时默认为 Mbps。' },
      { key: 'bbr-profile', label: 'BBR Profile', section: SECTION.quic, kind: 'select', options: choices('standard', 'conservative', 'aggressive') },
      { key: 'obfs', label: 'QUIC 混淆器', section: SECTION.protocol, kind: 'select', options: choices('salamander', 'gecko') },
      { key: 'obfs-password', label: '混淆密码', section: SECTION.protocol, kind: 'password', ...shown(truthy('obfs')) },
      { key: 'obfs-min-packet-size', label: 'Gecko 最小线上包', section: SECTION.protocol, kind: 'number', min: 0, description: '单位字节，仅 Gecko。', ...shown(equals('obfs', 'gecko')) },
      { key: 'obfs-max-packet-size', label: 'Gecko 最大线上包', section: SECTION.protocol, kind: 'number', min: 0, description: '单位字节，仅 Gecko。', ...shown(equals('obfs', 'gecko')) },
      ...tlsFields(),
      { key: 'realm-opts.enable', label: '启用 Realm', section: 'Realm', kind: 'boolean' },
      { key: 'realm-opts.server-url', label: 'Realm 服务 URL', section: 'Realm', placeholder: 'https://realm.hy2.io', ...shown(truthy('realm-opts.enable')) },
      { key: 'realm-opts.token', label: 'Realm Token', section: 'Realm', kind: 'password', ...shown(truthy('realm-opts.enable')) },
      { key: 'realm-opts.realm-id', label: 'Realm ID', section: 'Realm', ...shown(truthy('realm-opts.enable')) },
      { key: 'realm-opts.stun-servers', label: 'Realm STUN 服务器', section: 'Realm', kind: 'string-list', placeholder: 'stun.example.com:3478', ...shown(truthy('realm-opts.enable')) },
      { key: 'realm-opts.sni', label: 'Realm SNI', section: 'Realm TLS', ...shown(truthy('realm-opts.enable')) },
      { key: 'realm-opts.skip-cert-verify', label: 'Realm 跳过证书验证', section: 'Realm TLS', kind: 'boolean', ...shown(truthy('realm-opts.enable')) },
      { key: 'realm-opts.name-cert-verify', label: 'Realm 证书 DNSName', section: 'Realm TLS', ...shown(truthy('realm-opts.enable')) },
      { key: 'realm-opts.fingerprint', label: 'Realm 证书指纹', section: 'Realm TLS', ...shown(truthy('realm-opts.enable')) },
      { key: 'realm-opts.certificate', label: 'Realm mTLS 证书', section: 'Realm TLS', kind: 'textarea', ...shown(truthy('realm-opts.enable')) },
      { key: 'realm-opts.private-key', label: 'Realm mTLS 私钥', section: 'Realm TLS', kind: 'textarea', ...shown(truthy('realm-opts.enable')) },
      { key: 'realm-opts.alpn', label: 'Realm ALPN', section: 'Realm TLS', kind: 'string-list', ...shown(truthy('realm-opts.enable')) },
      { key: 'initial-stream-receive-window', label: '初始流接收窗口', section: SECTION.quic, kind: 'number', min: 0 },
      { key: 'max-stream-receive-window', label: '最大流接收窗口', section: SECTION.quic, kind: 'number', min: 0 },
      { key: 'initial-connection-receive-window', label: '初始连接接收窗口', section: SECTION.quic, kind: 'number', min: 0 },
      { key: 'max-connection-receive-window', label: '最大连接接收窗口', section: SECTION.quic, kind: 'number', min: 0 },
    ],
  },
  tuic: {
    label: 'TUIC',
    category: 'QUIC / VPN',
    endpoint: true,
    includeSmux: false,
    fields: [
      { key: 'token', label: 'TUIC v4 Token', section: SECTION.auth, kind: 'password', description: 'TUIC v4 必填；使用 v5 时不可填写。' },
      { key: 'uuid', label: 'TUIC v5 UUID', section: SECTION.auth, description: 'TUIC v5 必填；使用 v4 时不可填写。' },
      { key: 'password', label: 'TUIC v5 密码', section: SECTION.auth, kind: 'password', description: 'TUIC v5 必填；使用 v4 时不可填写。' },
      { key: 'ip', label: '服务器解析覆盖 IP', section: SECTION.server },
      { key: 'heartbeat-interval', label: '心跳间隔', section: SECTION.quic, kind: 'number', min: 0, description: '单位毫秒。' },
      { key: 'disable-sni', label: '禁用 SNI', section: SECTION.tls, kind: 'boolean' },
      { key: 'reduce-rtt', label: '启用 QUIC 0-RTT', section: SECTION.quic, kind: 'boolean' },
      { key: 'request-timeout', label: '请求超时', section: SECTION.quic, kind: 'number', min: 0, description: '单位毫秒。' },
      { key: 'udp-relay-mode', label: 'UDP Relay 模式', section: SECTION.quic, kind: 'select', options: choices('native', 'quic') },
      { key: 'congestion-controller', label: '拥塞控制', section: SECTION.quic, kind: 'select', options: choices('cubic', 'new_reno', 'bbr') },
      { key: 'bbr-profile', label: 'BBR Profile', section: SECTION.quic, kind: 'select', options: choices('standard', 'conservative', 'aggressive'), ...shown(equals('congestion-controller', 'bbr')) },
      { key: 'max-udp-relay-packet-size', label: '最大 UDP Relay 包', section: SECTION.quic, kind: 'number', min: 0, description: '单位字节。' },
      { key: 'fast-open', label: 'Fast Open', section: SECTION.quic, kind: 'boolean' },
      { key: 'max-open-streams', label: '最大打开流数', section: SECTION.quic, kind: 'number', min: 0 },
      ...tlsFields(),
    ],
  },
  wireguard: {
    label: 'WireGuard',
    category: 'QUIC / VPN',
    endpoint: true,
    includeSmux: false,
    endpointFields: [
      { ...MANUAL_NODE_ENDPOINT_FIELDS[0], showWhen: truthy('peers', false) },
      { ...MANUAL_NODE_ENDPOINT_FIELDS[1], showWhen: truthy('peers', false) },
    ],
    fields: [
      { key: 'ip', label: '本机 IPv4', section: SECTION.vpn, required: true, placeholder: '172.16.0.2' },
      { key: 'ipv6', label: '本机 IPv6', section: SECTION.vpn, placeholder: 'fd00::2' },
      { key: 'private-key', label: '客户端私钥', section: SECTION.auth, kind: 'password', required: true },
      { key: 'public-key', label: '服务端公钥', section: SECTION.auth, required: true, ...shown(truthy('peers', false)) },
      { key: 'allowed-ips', label: 'Allowed IPs', section: SECTION.vpn, kind: 'string-list', placeholder: '0.0.0.0/0', ...shown(truthy('peers', false)) },
      { key: 'pre-shared-key', label: '预共享密钥', section: SECTION.auth, kind: 'password', ...shown(truthy('peers', false)) },
      { key: 'reserved', label: 'Reserved', section: SECTION.vpn, placeholder: '209,98,59 或 U4An', description: '官方同时接受数组和字符串；文本形式可覆盖两种来源格式。', ...shown(truthy('peers', false)) },
      { key: 'persistent-keepalive', label: 'Persistent Keepalive', section: SECTION.vpn, kind: 'number', min: 0, ...shown(truthy('peers', false)) },
      {
        key: 'peers',
        label: 'WireGuard Peers',
        section: SECTION.vpn,
        kind: 'object-list',
        description: '完整写法；填写后顶层 server、port、公钥、预共享密钥和 Reserved 等 peer 字段会被忽略。',
        itemFields: [
          { key: 'server', label: '服务器', section: 'WireGuard Peer', required: true },
          { key: 'port', label: '端口', section: 'WireGuard Peer', kind: 'number', required: true, min: 1, max: 65535 },
          { key: 'public-key', label: '服务端公钥', section: 'WireGuard Peer', required: true },
          { key: 'allowed-ips', label: 'Allowed IPs', section: 'WireGuard Peer', kind: 'string-list', description: '多个 peer 时，各 peer 的网段需要区分。' },
          { key: 'pre-shared-key', label: '预共享密钥', section: 'WireGuard Peer', kind: 'password' },
          { key: 'reserved', label: 'Reserved', section: 'WireGuard Peer', placeholder: '209,98,59 或 U4An' },
          { key: 'persistent-keepalive', label: 'Persistent Keepalive', section: 'WireGuard Peer', kind: 'number', min: 0 },
        ],
      },
      { key: 'mtu', label: 'MTU', section: SECTION.vpn, kind: 'number', min: 0 },
      { key: 'remote-dns-resolve', label: '强制远程 DNS', section: SECTION.dns, kind: 'boolean' },
      { key: 'dns', label: '远程 DNS 服务器', section: SECTION.dns, kind: 'string-list', ...shown(truthy('remote-dns-resolve')) },
      { key: 'amnezia-wg-option.jc', label: 'Amnezia JC', section: 'AmneziaWG', kind: 'number', min: 0, step: 1 },
      { key: 'amnezia-wg-option.jmin', label: 'Amnezia JMin', section: 'AmneziaWG', kind: 'number', min: 0, step: 1 },
      { key: 'amnezia-wg-option.jmax', label: 'Amnezia JMax', section: 'AmneziaWG', kind: 'number', min: 0, step: 1 },
      { key: 'amnezia-wg-option.s1', label: 'Amnezia S1', section: 'AmneziaWG', kind: 'number', min: 0, step: 1 },
      { key: 'amnezia-wg-option.s2', label: 'Amnezia S2', section: 'AmneziaWG', kind: 'number', min: 0, step: 1 },
      { key: 'amnezia-wg-option.s3', label: 'Amnezia S3', section: 'AmneziaWG', kind: 'number', min: 0, step: 1, description: 'AmneziaWG v1.5 / v2。' },
      { key: 'amnezia-wg-option.s4', label: 'Amnezia S4', section: 'AmneziaWG', kind: 'number', min: 0, step: 1, description: 'AmneziaWG v1.5 / v2。' },
      { key: 'amnezia-wg-option.h1', label: 'Amnezia H1', section: 'AmneziaWG', placeholder: '123456 或 123456-123500', description: 'v2 可填写范围。' },
      { key: 'amnezia-wg-option.h2', label: 'Amnezia H2', section: 'AmneziaWG', placeholder: '67543 或 67543-67550', description: 'v2 可填写范围。' },
      { key: 'amnezia-wg-option.h3', label: 'Amnezia H3', section: 'AmneziaWG', placeholder: '123123 或 123123-123200', description: 'v2 可填写范围。' },
      { key: 'amnezia-wg-option.h4', label: 'Amnezia H4', section: 'AmneziaWG', placeholder: '32345 或 32345-32350', description: 'v2 可填写范围。' },
      { key: 'amnezia-wg-option.i1', label: 'Amnezia I1', section: 'AmneziaWG', kind: 'textarea', description: 'AmneziaWG v1.5 / v2 指令串。' },
      { key: 'amnezia-wg-option.i2', label: 'Amnezia I2', section: 'AmneziaWG', kind: 'textarea', description: 'AmneziaWG v1.5 / v2 指令串。' },
      { key: 'amnezia-wg-option.i3', label: 'Amnezia I3', section: 'AmneziaWG', kind: 'textarea', description: 'AmneziaWG v1.5 / v2 指令串。' },
      { key: 'amnezia-wg-option.i4', label: 'Amnezia I4', section: 'AmneziaWG', kind: 'textarea', description: 'AmneziaWG v1.5 / v2 指令串。' },
      { key: 'amnezia-wg-option.i5', label: 'Amnezia I5', section: 'AmneziaWG', kind: 'textarea', description: 'AmneziaWG v1.5 / v2 指令串。' },
      { key: 'amnezia-wg-option.j1', label: 'Amnezia J1', section: 'AmneziaWG', kind: 'textarea', description: '仅 AmneziaWG v1.5。' },
      { key: 'amnezia-wg-option.j2', label: 'Amnezia J2', section: 'AmneziaWG', kind: 'textarea', description: '仅 AmneziaWG v1.5。' },
      { key: 'amnezia-wg-option.j3', label: 'Amnezia J3', section: 'AmneziaWG', kind: 'textarea', description: '仅 AmneziaWG v1.5。' },
      { key: 'amnezia-wg-option.itime', label: 'Amnezia ITime', section: 'AmneziaWG', kind: 'number', min: 0, step: 1, description: '仅 AmneziaWG v1.5。' },
    ],
  },
  masque: {
    label: 'MASQUE',
    category: 'QUIC / VPN',
    endpoint: true,
    includeSmux: false,
    fields: [
      { key: 'private-key', label: 'ECDSA 客户端私钥', section: SECTION.auth, kind: 'password', required: true, description: 'base64 编码，不含 PEM 头尾与换行。' },
      { key: 'public-key', label: 'ECDSA 服务端公钥', section: SECTION.auth, required: true, description: 'base64 编码，不含 PEM 头尾与换行。' },
      { key: 'ip', label: '本机 IPv4 CIDR', section: SECTION.vpn, placeholder: '172.16.0.2/32' },
      { key: 'ipv6', label: '本机 IPv6 CIDR', section: SECTION.vpn, placeholder: 'fd00::2/128' },
      { key: 'mtu', label: 'MTU', section: SECTION.vpn, kind: 'number', min: 0 },
      { key: 'sni', label: 'SNI', section: SECTION.tls },
      { key: 'network', label: '传输模式', section: SECTION.protocol, kind: 'select', options: choices('quic', 'h2', 'h3-l4proxy'), description: '默认 quic；h3-l4proxy 当前不支持 UDP。' },
      { key: 'remote-dns-resolve', label: '强制远程 DNS', section: SECTION.dns, kind: 'boolean' },
      { key: 'dns', label: '远程 DNS 服务器', section: SECTION.dns, kind: 'string-list', ...shown(truthy('remote-dns-resolve')) },
      { key: 'congestion-controller', label: '拥塞控制', section: SECTION.quic, kind: 'select', options: choices('bbr') },
      { key: 'bbr-profile', label: 'BBR Profile', section: SECTION.quic, kind: 'select', options: choices('standard', 'conservative', 'aggressive'), ...shown(equals('congestion-controller', 'bbr')) },
      { key: 'handshake-timeout', label: '握手超时', section: SECTION.advanced, kind: 'number', min: 0, description: '单位秒；0 仅使用外层连接超时。' },
    ],
  },
  openvpn: {
    label: 'OpenVPN',
    category: 'QUIC / VPN',
    endpoint: true,
    includeSmux: false,
    fields: [
      { key: 'proto', label: '隧道传输协议', section: SECTION.protocol, kind: 'select', options: choices('udp', 'tcp') },
      { key: 'username', label: '用户名', section: SECTION.auth, description: '与 password 配对；用户名/密码认证和 cert/key 认证至少配置一组。' },
      { key: 'password', label: '密码', section: SECTION.auth, kind: 'password', description: '与 username 配对；用户名/密码认证和 cert/key 认证至少配置一组。' },
      { key: 'ca', label: 'CA 证书', section: SECTION.auth, kind: 'textarea', required: true, description: '复制 .ovpn 文件 <ca> 标签内的内容。' },
      { key: 'cert', label: '客户端证书', section: SECTION.auth, kind: 'textarea', description: '与 key 配对；使用用户名/密码认证时可省略。' },
      { key: 'key', label: '客户端私钥', section: SECTION.auth, kind: 'textarea', description: '与 cert 配对；使用用户名/密码认证时可省略。' },
      { key: 'tls-auth', label: 'TLS Auth Key', section: SECTION.tls, kind: 'textarea', description: '与 tls-crypt、tls-crypt-v2 互斥。' },
      { key: 'key-direction', label: 'TLS Auth Key Direction', section: SECTION.tls, kind: 'select', options: choices('0', '1'), description: '留空为双向模式。', ...shown(truthy('tls-auth')) },
      { key: 'tls-crypt', label: 'TLS Crypt Key', section: SECTION.tls, kind: 'textarea', description: '与 tls-auth、tls-crypt-v2 互斥。' },
      { key: 'tls-crypt-v2', label: 'TLS Crypt v2 Client Key', section: SECTION.tls, kind: 'textarea', description: '与 tls-auth、tls-crypt 互斥。' },
      { key: 'ping', label: 'Ping 间隔', section: SECTION.advanced, kind: 'number', min: 0 },
      { key: 'ping-restart', label: 'Ping Restart', section: SECTION.advanced, kind: 'number', min: 0 },
      { key: 'peer-info', label: 'Peer Info', section: SECTION.advanced, kind: 'key-value-list', valueKind: 'text', description: '追加在内置 IV_VER / IV_PROTO / IV_CIPHERS 后发送给服务端。' },
      { key: 'handshake-timeout', label: '握手超时', section: SECTION.advanced, kind: 'number', min: 0, description: '单位秒；0 仅使用外层连接超时。' },
      { key: 'dev', label: '虚拟网卡类型', section: SECTION.vpn, kind: 'select', options: choices('tun') },
      { key: 'cipher', label: '数据加密方式', section: SECTION.protocol, kind: 'select', options: COMMON_CIPHER_OPTIONS },
      { key: 'data-ciphers', label: '数据通道 Cipher 协商列表', section: SECTION.protocol, kind: 'string-list', placeholder: 'AES-256-GCM\nAES-128-GCM' },
      { key: 'data-ciphers-fallback', label: 'Cipher 协商回退', section: SECTION.protocol, kind: 'select', options: COMMON_CIPHER_OPTIONS },
      { key: 'auth', label: '数据验证算法', section: SECTION.protocol, kind: 'select', options: choices('MD5', 'SHA1', 'SHA256', 'SHA384', 'SHA512'), description: 'AEAD cipher 会忽略此项。' },
      { key: 'comp-lzo', label: 'LZO 压缩', section: SECTION.protocol, kind: 'select', options: choices('yes', 'no', 'adaptive') },
      { key: 'mtu', label: 'MTU', section: SECTION.vpn, kind: 'number', min: 0 },
      { key: 'remote-dns-resolve', label: '强制远程 DNS', section: SECTION.dns, kind: 'boolean' },
      { key: 'dns', label: '远程 DNS 服务器', section: SECTION.dns, kind: 'string-list', ...shown(truthy('remote-dns-resolve')) },
    ],
  },
  shadowquic: {
    label: 'ShadowQUIC',
    category: 'QUIC / VPN',
    endpoint: true,
    includeSmux: false,
    fields: [
      { key: 'username', label: '用户名', section: SECTION.auth, required: true },
      { key: 'password', label: '密码', section: SECTION.auth, kind: 'password', required: true },
      ...tlsFields(),
      { key: 'quic-versions', label: 'QUIC 版本', section: SECTION.quic, kind: 'string-list', description: '可选 v1 / v2，默认 v1。' },
      { key: 'udp-over-stream', label: 'UDP over Stream', section: SECTION.quic, kind: 'boolean' },
      { key: 'zero-rtt', label: '启用 0-RTT', section: SECTION.quic, kind: 'boolean' },
      { key: 'keep-alive-interval', label: '保活间隔', section: SECTION.quic, kind: 'number', min: 0, description: '单位毫秒。' },
      { key: 'congestion-controller', label: '拥塞控制', section: SECTION.quic, kind: 'select', options: choices('cubic', 'new_reno', 'bbr') },
      { key: 'up', label: 'Brutal 上行带宽', section: SECTION.quic, placeholder: '100 Mbps', description: '填写后使用 Mihomo 私有扩展协商 Brutal。' },
      { key: 'down', label: 'Brutal 下行带宽', section: SECTION.quic, placeholder: '100 Mbps', description: '填写后使用 Mihomo 私有扩展协商 Brutal。' },
      { key: 'cwnd', label: '初始拥塞窗口', section: SECTION.quic, kind: 'number', min: 0 },
      { key: 'bbr-profile', label: 'BBR Profile', section: SECTION.quic, kind: 'select', options: choices('standard', 'conservative', 'aggressive'), ...shown(equals('congestion-controller', 'bbr')) },
      { key: 'max-datagram-frame-size', label: '最大 Datagram Frame', section: SECTION.quic, kind: 'number', min: 0, description: '单位字节。' },
      { key: 'max-open-streams', label: '最大并发流数', section: SECTION.quic, kind: 'number', min: 0 },
      { key: 'recv-window-conn', label: '流接收窗口', section: SECTION.quic, kind: 'number', min: 0 },
      { key: 'recv-window', label: '连接接收窗口', section: SECTION.quic, kind: 'number', min: 0 },
      { key: 'disable-mtu-discovery', label: '禁用 MTU 探测', section: SECTION.quic, kind: 'boolean' },
    ],
  },
  mieru: {
    label: 'Mieru',
    category: 'QUIC / VPN',
    endpoint: true,
    includeSmux: false,
    endpointFields: [
      MANUAL_NODE_ENDPOINT_FIELDS[0],
      { ...MANUAL_NODE_ENDPOINT_FIELDS[1], showWhen: truthy('port-range', false), description: '与 port-range 二选一。' },
    ],
    fields: [
      { key: 'port-range', label: '端口范围', section: SECTION.server, placeholder: '2090-2099', description: '与 port 二选一。', ...shown(truthy('port', false)) },
      { key: 'transport', label: '传输协议', section: SECTION.protocol, kind: 'select', options: choices('TCP', 'UDP') },
      { key: 'username', label: '用户名', section: SECTION.auth, required: true },
      { key: 'password', label: '密码', section: SECTION.auth, kind: 'password', required: true },
      { key: 'multiplexing', label: '多路复用', section: SECTION.protocol, kind: 'select', options: choices('MULTIPLEXING_OFF', 'MULTIPLEXING_LOW', 'MULTIPLEXING_MIDDLE', 'MULTIPLEXING_HIGH') },
      { key: 'handshake-mode', label: '握手模式', section: SECTION.protocol, kind: 'select', options: choices('HANDSHAKE_STANDARD', 'HANDSHAKE_NO_WAIT') },
      { key: 'traffic-pattern', label: 'Traffic Pattern', section: SECTION.protocol, kind: 'textarea', description: '用于微调网络行为的 base64 字符串。' },
    ],
  },
  anytls: {
    label: 'AnyTLS',
    category: '常用协议',
    endpoint: true,
    fields: [
      { key: 'password', label: '密码', section: SECTION.auth, kind: 'password', required: true },
      { key: 'idle-session-check-interval', label: '空闲会话检查间隔', section: SECTION.protocol, kind: 'number', min: 0, description: '单位秒。' },
      { key: 'idle-session-timeout', label: '空闲会话超时', section: SECTION.protocol, kind: 'number', min: 0, description: '单位秒。' },
      { key: 'min-idle-session', label: '最小空闲会话数', section: SECTION.protocol, kind: 'number', min: 0, step: 1 },
      ...tlsFields({ clientFingerprint: true, carrier: true }),
    ],
  },
  sudoku: {
    label: 'Sudoku',
    category: '特殊出站',
    endpoint: true,
    fields: [
      { key: 'key', label: '客户端密钥', section: SECTION.auth, kind: 'password', required: true, description: 'ED25519 私钥，或与服务端相同的 UUID。' },
      { key: 'aead-method', label: 'AEAD 算法', section: SECTION.protocol, kind: 'select', options: choices('chacha20-poly1305', 'aes-128-gcm', 'none'), description: 'none 不提供 AEAD 保护。' },
      { key: 'padding-min', label: '最小填充率', section: SECTION.protocol, kind: 'number', min: 0, max: 100 },
      { key: 'padding-max', label: '最大填充率', section: SECTION.protocol, kind: 'number', min: 0, max: 100, description: '必须大于或等于 padding-min。' },
      { key: 'table-type', label: '字节表类型', section: SECTION.protocol, kind: 'select', options: choices('prefer_ascii', 'prefer_entropy', 'up_ascii_down_entropy', 'up_entropy_down_ascii') },
      { key: 'custom-table', label: '自定义字节表', section: SECTION.protocol, description: '必须包含 2 个 x、2 个 p 和 4 个 v；只对 entropy 方向生效。' },
      { key: 'custom-tables', label: '自定义字节表列表', section: SECTION.protocol, kind: 'string-list', description: '非空时覆盖 custom-table。' },
      { key: 'multiplex', label: '多路复用', section: SECTION.protocol, kind: 'select', options: choices('off', 'auto', 'on') },
      { key: 'httpmask.disable', label: '禁用 HTTPMask', section: 'HTTPMask', kind: 'boolean' },
      { key: 'httpmask.mode', label: 'HTTPMask 模式', section: 'HTTPMask', kind: 'select', options: choices('legacy', 'stream', 'poll', 'auto', 'ws'), ...shown(truthy('httpmask.disable', false)) },
      { key: 'httpmask.tls', label: 'HTTPMask TLS', section: 'HTTPMask', kind: 'boolean', description: '仅 stream/poll/auto/ws；true 强制 HTTPS，false 强制 HTTP。', ...shown(oneOf('httpmask.mode', 'stream', 'poll', 'auto', 'ws')) },
      { key: 'httpmask.host', label: 'HTTPMask Host / SNI', section: 'HTTPMask', placeholder: 'example.com:443', ...shown(oneOf('httpmask.mode', 'stream', 'poll', 'auto', 'ws')) },
      { key: 'httpmask.path-root', label: 'HTTPMask 路径前缀', section: 'HTTPMask', placeholder: 'aabbcc', ...shown(truthy('httpmask.disable', false)) },
      { key: 'httpmask.multiplex', label: 'HTTPMask 多路复用', section: 'HTTPMask', kind: 'select', options: choices('off', 'auto', 'on'), description: '兼容旧配置；填写时优先于顶层 multiplex。', ...shown(truthy('httpmask.disable', false)) },
      { key: 'enable-pure-downlink', label: '纯 Sudoku 下行', section: SECTION.protocol, kind: 'boolean', description: '需要与服务端保持一致。' },
    ],
  },
  trusttunnel: {
    label: 'TrustTunnel',
    category: '特殊出站',
    endpoint: true,
    includeSmux: false,
    fields: [
      { key: 'username', label: '用户名', section: SECTION.auth, required: true },
      { key: 'password', label: '密码', section: SECTION.auth, kind: 'password', required: true },
      { key: 'health-check', label: '健康检查', section: SECTION.protocol, kind: 'boolean' },
      ...tlsFields({ clientFingerprint: true }),
      { key: 'quic', label: '使用 QUIC', section: SECTION.quic, kind: 'boolean' },
      { key: 'congestion-controller', label: 'QUIC 拥塞控制', section: SECTION.quic, kind: 'select', options: choices('cubic', 'new_reno', 'bbr'), ...shown(truthy('quic')) },
      { key: 'bbr-profile', label: 'BBR Profile', section: SECTION.quic, kind: 'select', options: choices('standard', 'conservative', 'aggressive'), ...shown(equals('congestion-controller', 'bbr')) },
      { key: 'max-connections', label: '最大连接数', section: SECTION.protocol, kind: 'number', min: 0, description: '与 max-streams 冲突。' },
      { key: 'min-streams', label: '新建连接前最小流数', section: SECTION.protocol, kind: 'number', min: 0, description: '与 max-streams 冲突。' },
      { key: 'max-streams', label: '新建连接前最大流数', section: SECTION.protocol, kind: 'number', min: 0, description: '与 max-connections 和 min-streams 冲突。' },
    ],
  },
  tailscale: {
    label: 'Tailscale',
    category: '特殊出站',
    endpoint: false,
    fields: [
      { key: 'hostname', label: '设备名', section: SECTION.protocol, placeholder: 'mihomo' },
      { key: 'auth-key', label: 'Auth Key', section: SECTION.auth, kind: 'password', description: '留空时首次启动会在日志输出交互式登录 URL。' },
      { key: 'control-url', label: 'Control Server', section: SECTION.server, placeholder: 'https://controlplane.tailscale.com' },
      { key: 'state-dir', label: '状态目录', section: SECTION.protocol, placeholder: './tailscale' },
      { key: 'ephemeral', label: 'Ephemeral Node', section: SECTION.protocol, kind: 'boolean' },
      { key: 'udp', label: '允许 UDP', section: SECTION.network, kind: 'boolean' },
      { key: 'accept-routes', label: '接受 Subnet Routes', section: SECTION.vpn, kind: 'boolean' },
      { key: 'exit-node', label: 'Exit Node', section: SECTION.vpn, placeholder: '100.64.0.1 或 auto:any' },
      { key: 'exit-node-allow-lan-access', label: 'Exit Node 允许 LAN', section: SECTION.vpn, kind: 'boolean', ...shown(truthy('exit-node')) },
      { key: 'dialer-proxy', label: '前置代理', section: SECTION.network, placeholder: '节点或策略组名称' },
      { key: 'interface-name', label: '绑定网卡', section: SECTION.network },
      { key: 'routing-mark', label: '路由标记', section: SECTION.network, kind: 'number' },
      { key: 'ip-version', label: '出站 IP 版本', section: SECTION.network, kind: 'select', options: IP_VERSION_OPTIONS },
    ],
  },
  direct: {
    label: 'Direct',
    category: '特殊出站',
    endpoint: false,
    fields: [
      { key: 'udp', label: '允许 UDP', section: SECTION.network, kind: 'boolean' },
      { key: 'ip-version', label: '出站 IP 版本', section: SECTION.network, kind: 'select', options: IP_VERSION_OPTIONS },
      { key: 'interface-name', label: '绑定网卡', section: SECTION.network },
      { key: 'routing-mark', label: '路由标记', section: SECTION.network, kind: 'number' },
    ],
  },
  dns: {
    label: 'DNS',
    category: '特殊出站',
    endpoint: false,
    fields: [],
  },
  rematch: {
    label: 'Rematch',
    category: '特殊出站',
    endpoint: false,
    fields: [
      { key: 'target-rematch-name', label: 'Rematch 标记', section: SECTION.protocol, placeholder: 'streaming', description: '覆盖 metadata 中的 rematch-name，再次进入规则匹配。' },
      { key: 'target-sub-rule', label: '目标子规则', section: SECTION.protocol, placeholder: 'ai-rules', description: '名称不存在或为空时回退到主 rules。' },
    ],
  },
};

export const MANUAL_NODE_PROTOCOL_ENTRIES = Object.entries(MANUAL_NODE_PROTOCOLS);

export function fieldsForProtocol(type: string): ManualNodeField[] {
  const protocol = MANUAL_NODE_PROTOCOLS[type];
  if (!protocol) return [];
  const endpointFields = protocol.endpointFields ?? (protocol.endpoint ? MANUAL_NODE_ENDPOINT_FIELDS : []);
  const commonFields = protocol.includeCommon === false || !protocol.endpoint ? [] : MANUAL_NODE_COMMON_FIELDS;
  const smuxFields = protocol.includeSmux === false || !protocol.endpoint ? [] : MANUAL_NODE_SMUX_FIELDS;
  const seen = new Set<string>();
  return [...endpointFields, ...protocol.fields, ...commonFields, ...smuxFields].filter(field => {
    if (seen.has(field.key)) return false;
    seen.add(field.key);
    return true;
  });
}
