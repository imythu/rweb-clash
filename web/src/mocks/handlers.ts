import { http, HttpResponse, delay } from 'msw'

// 在内存中保存模拟的后端状态
let systemConfig = {
  mode: 'rule',
  tun: true,
  system_proxy: true,
  global_kill: false,
  http_port: 7890,
  socks_port: 7891,
  mixed_port: 7890,
  external_controller: '9090',
  external_controller_enabled: true,
  ipv6: true,
  allow_lan: false,
  dns_enabled: true,
  dns_mode: 'fake-ip',
  store_selected: true,
  unified_delay: true,
  tcp_concurrent: false
};

const SUB_DELIMITER = ' ^_^ ';

// 模拟代理组和节点数据
interface ProxyGroup {
  name: string;
  type: string;
  now: string;
  delay: number;
  all: string[];
  source?: 'local' | 'subscription'; // 来源区分
}

let proxies: Record<string, ProxyGroup> = {
  "🚀 自动选择": { 
    name: "🚀 自动选择", 
    type: "url-test", 
    now: `香港 01 (专线)${SUB_DELIMITER}SpeedFly`, 
    delay: 24, 
    all: [`香港 01 (专线)${SUB_DELIMITER}SpeedFly`, `香港 02${SUB_DELIMITER}SpeedFly`, `新加坡 01${SUB_DELIMITER}FlowerCloud`],
    source: 'local'
  },
  "🎬 流媒体": { 
    name: "🎬 流媒体", 
    type: "select", 
    now: `美国 05${SUB_DELIMITER}FlowerCloud`, 
    delay: 156, 
    all: [`美国 05${SUB_DELIMITER}FlowerCloud`, `美国 06${SUB_DELIMITER}FlowerCloud`, "🚀 自动选择"],
    source: 'local'
  },
  "🎮 游戏加速": { 
    name: "🎮 游戏加速", 
    type: "fallback", 
    now: `日本 02${SUB_DELIMITER}SpeedFly`, 
    delay: 45, 
    all: [`日本 02${SUB_DELIMITER}SpeedFly`, `香港 01 (专线)${SUB_DELIMITER}SpeedFly`],
    source: 'subscription' // 模拟订阅导入
  },
  "⚖️ 负载均衡": {
    name: "⚖️ 负载均衡",
    type: "load-balance",
    now: "multiple",
    delay: 35,
    all: [`香港 01 (专线)${SUB_DELIMITER}SpeedFly`, `香港 02${SUB_DELIMITER}SpeedFly`],
    source: 'local'
  },
  "📁 电报专用": { 
    name: "📁 电报专用", 
    type: "select", 
    now: `新加坡 01${SUB_DELIMITER}FlowerCloud`, 
    delay: 89, 
    all: [`新加坡 01${SUB_DELIMITER}FlowerCloud`, `美国 05${SUB_DELIMITER}FlowerCloud`],
    source: 'local'
  }
};

let allNodes = [
  { name: `香港 01 (专线)${SUB_DELIMITER}SpeedFly`, type: "Shadowsocks", latency: 24, country: "HK" },
  { name: `香港 02${SUB_DELIMITER}SpeedFly`, type: "Shadowsocks", latency: 32, country: "HK" },
  { name: `日本 02${SUB_DELIMITER}SpeedFly`, type: "Vmess", latency: 45, country: "JP" },
  { name: `新加坡 01${SUB_DELIMITER}FlowerCloud`, type: "Trojan", latency: 89, country: "SG" },
  { name: `美国 05${SUB_DELIMITER}FlowerCloud`, type: "Hysteria2", latency: 156, country: "US" },
  { name: `美国 06${SUB_DELIMITER}FlowerCloud`, type: "Hysteria2", latency: 162, country: "US" },
];

// --- 扩展 10 倍数据用于大规模测试 ---
const extraNodes: any[] = [];
for (let i = 1; i <= 20; i++) {
  allNodes.forEach(node => {
    const [label, sub] = node.name.split(SUB_DELIMITER);
    const clone = { ...node, name: `${label} (区域 ${i})${SUB_DELIMITER}${sub}` };
    clone.latency = Math.max(5, node.latency + Math.floor(Math.random() * 60 - 30));
    extraNodes.push(clone);
  });
}
allNodes = [...allNodes, ...extraNodes];

Object.keys(proxies).forEach(groupName => {
  const group = proxies[groupName];
  const newAll = [...group.all];
  group.all.forEach(nodeName => {
    if (!nodeName.includes(SUB_DELIMITER)) return;
    const [label, sub] = nodeName.split(SUB_DELIMITER);
    for (let i = 1; i <= 20; i++) {
      newAll.push(`${label} (区域 ${i})${SUB_DELIMITER}${sub}`);
    }
  });
  group.all = newAll;
});

// 生成大量额外的分组
for (let i = 1; i <= 30; i++) {
  proxies[`🌐 外部订阅 ${i}`] = {
    name: `🌐 外部订阅 ${i}`,
    type: i % 2 === 0 ? 'url-test' : 'fallback',
    now: allNodes[i % allNodes.length].name,
    delay: allNodes[i % allNodes.length].latency,
    all: allNodes.map(n => n.name).slice(i, i + 30), // 每个组随机塞点
    source: 'subscription'
  };
}
for (let i = 1; i <= 15; i++) {
  proxies[`🛠️ 业务隔离区 ${i}`] = {
    name: `🛠️ 业务隔离区 ${i}`,
    type: 'select',
    now: allNodes[0].name,
    delay: allNodes[0].latency,
    all: allNodes.map(n => n.name).slice(0, 40),
    source: 'local'
  };
}
// --- 扩展结束 ---

// 模拟订阅数据
let subscriptions = [
  { 
    id: '1', 
    name: "SpeedFly", 
    url: "https://sub.speedfly.xyz/link/...", 
    nodes: 142, 
    traffic: { used: 45.2 * 1024 * 1024 * 1024, total: 200 * 1024 * 1024 * 1024 },
    expiry: "2026-12-25",
    interval: 360,
    inheritGlobal: true,
    breakdown: { "SS": 80, "Trojan": 42, "Hy2": 20 },
    lastUpdate: "10 分钟前", 
    status: "online",
    rules: [
      { type: 'regex', pattern: '.*香港.*', action: 'keep' }
    ]
  },
  { 
    id: '2', 
    name: "FlowerCloud", 
    url: "https://flower.cloud/api/v1/...", 
    nodes: 86, 
    traffic: { used: 120.5 * 1024 * 1024 * 1024, total: 150 * 1024 * 1024 * 1024 },
    expiry: "2026-05-12",
    interval: 1440,
    inheritGlobal: true,
    breakdown: { "Vmess": 50, "SS": 36 },
    lastUpdate: "2 小时前", 
    status: "online",
    rules: []
  }
];

// 模拟路由规则数据
let rules = [
  { id: '1', type: 'DOMAIN-SUFFIX', value: 'google.com', policy: '🚀 自动选择', desc: 'Google 全家桶' },
  { id: '2', type: 'DOMAIN-KEYWORD', value: 'netflix', policy: '🎬 流媒体', desc: 'Netflix 视频' },
  { id: '3', type: 'IP-CIDR', value: '192.168.1.0/24', policy: 'DIRECT', desc: '局域网访问' },
  { id: '4', type: 'GEOIP', value: 'CN', policy: 'DIRECT', desc: '中国大陆流量' },
  { id: '5', type: 'MATCH', value: 'ANY', policy: '🐟 漏网之鱼', desc: '默认兜底规则' },
];

// 模拟日志数据
let logs = [
  { time: '2024-04-26 10:00:01', level: 'info', payload: 'Clash for Mihomo v1.18.0 starting...' },
  { time: '2024-04-26 10:00:02', level: 'info', payload: 'Initial configuration file loaded successfully' },
  { time: '2024-04-26 10:00:03', level: 'warning', payload: 'Rule [DOMAIN-KEYWORD, ads] has no matched proxy, skipping' },
  { time: '2024-04-26 10:00:05', level: 'info', payload: 'Inbound [HTTP] listening at: 127.0.0.1:7890' },
  { time: '2024-04-26 10:00:05', level: 'info', payload: 'Inbound [SOCKS5] listening at: 127.0.0.1:7891' },
  { time: '2024-04-26 10:00:08', level: 'error', payload: 'Failed to update subscription [SpeedFly]: Network Timeout' },
  { time: '2024-04-26 10:01:20', level: 'info', payload: '[TCP] 127.0.0.1:54321 --> google.com:443 match DomainSuffix(google.com) using 🚀 自动选择[香港 01 (专线)]' },
  { time: '2024-04-26 10:02:15', level: 'info', payload: '[UDP] 127.0.0.1:61234 --> 8.8.8.8:53 match Match() using DIRECT' },
];

// 模拟规则集数据
let ruleSets = [
  { id: 'rs1', name: 'Loyalsoldier Domain', type: 'http', behavior: 'domain', format: 'text', url: 'https://raw.githubusercontent.com/...', interval: 86400, ruleCount: 12450, lastUpdate: '1 天前' },
  { id: 'rs2', name: 'Loyalsoldier IP', type: 'http', behavior: 'ipcidr', format: 'text', url: 'https://raw.githubusercontent.com/...', interval: 86400, ruleCount: 3500, lastUpdate: '3 小时前' },
];

export const handlers = [
  // 获取路由规则
  http.get('/api/rules', async () => {
    await delay(300);
    return HttpResponse.json(rules);
  }),

  // 获取规则集
  http.get('/api/rule-sets', async () => {
    await delay(300);
    return HttpResponse.json(ruleSets);
  }),

  // 添加规则集
  http.post('/api/rule-sets', async ({ request }) => {
    const data = await request.json() as any;
    const newRS = {
      id: Math.random().toString(36).substr(2, 9),
      ruleCount: 0,
      lastUpdate: '从未',
      ...data
    };
    ruleSets.push(newRS);
    return HttpResponse.json(newRS, { status: 201 });
  }),

  // 刷新规则集
  http.post('/api/rule-sets/:id/refresh', async () => {
    await delay(1500);
    return new HttpResponse(null, { status: 204 });
  }),

  // 删除规则集
  http.delete('/api/rule-sets/:id', async ({ params }) => {
    const { id } = params;
    ruleSets = ruleSets.filter(rs => rs.id !== id);
    return new HttpResponse(null, { status: 204 });
  }),

  // 添加路由规则
  http.post('/api/rules', async ({ request }) => {
    const data = await request.json() as any;
    const newRule = {
      id: Math.random().toString(36).substr(2, 9),
      ...data
    };
    rules.unshift(newRule);
    return HttpResponse.json(newRule, { status: 201 });
  }),

  // 更新路由规则
  http.put('/api/rules/:id', async ({ params, request }) => {
    const { id } = params;
    const updates = await request.json() as any;
    const index = rules.findIndex(r => r.id === id);
    if (index !== -1) {
      rules[index] = { ...rules[index], ...updates };
      return HttpResponse.json(rules[index]);
    }
    return new HttpResponse(null, { status: 404 });
  }),

  // 删除路由规则
  http.delete('/api/rules/:id', async ({ params }) => {
    const { id } = params;
    rules = rules.filter(r => r.id !== id);
    return new HttpResponse(null, { status: 204 });
  }),

  // 模拟规则沙盒测试
  http.post('/api/rules/test', async ({ request }) => {
    const { target } = await request.json() as { target: string };
    await delay(600);
    if (target.includes('google')) {
      return HttpResponse.json({ hitRule: rules[0], finalProxy: `香港 05 (IEPL)${SUB_DELIMITER}Fly` });
    }
    return HttpResponse.json({ hitRule: rules[4], finalProxy: `日本 02${SUB_DELIMITER}Cloud` });
  }),

  http.get('/api/subscriptions', async () => {
    await delay(400);
    return HttpResponse.json(subscriptions);
  }),

  // 添加订阅
  http.post('/api/subscriptions', async ({ request }) => {
    const data = await request.json() as any;
    const newSub = {
      id: Math.random().toString(36).substr(2, 9),
      nodes: 0,
      traffic: { used: 0, total: 100 * 1024 * 1024 * 1024 },
      expiry: "N/A",
      interval: 360,
      inheritGlobal: true,
      breakdown: {},
      lastUpdate: '刚刚',
      status: 'online',
      rules: [],
      ...data
    };
    subscriptions.push(newSub);
    return HttpResponse.json(newSub, { status: 201 });
  }),

  // 更新订阅
  http.patch('/api/subscriptions/:id', async ({ params, request }) => {
    const { id } = params;
    const updates = await request.json() as any;
    const index = subscriptions.findIndex(s => s.id === id);
    if (index !== -1) {
      subscriptions[index] = { ...subscriptions[index], ...updates };
      return HttpResponse.json(subscriptions[index]);
    }
    return new HttpResponse(null, { status: 404 });
  }),

  // 删除订阅
  http.delete('/api/subscriptions/:id', async ({ params }) => {
    const { id } = params;
    subscriptions = subscriptions.filter(s => s.id !== id);
    return new HttpResponse(null, { status: 204 });
  }),

  http.get('/api/proxies', async () => {
    await delay(300);
    return HttpResponse.json({ groups: Object.values(proxies), nodes: allNodes });
  }),

  // 切换代理组选中的节点
  http.put('/api/proxies/:group', async ({ params, request }) => {
    const { group } = params;
    const { name } = await request.json() as { name: string };
    if (proxies[group as string]) {
      proxies[group as string].now = name;
      // 模拟延迟变化
      const node = allNodes.find(n => n.name === name);
      proxies[group as string].delay = node ? node.latency : 0;
    }
    await delay(500);
    return HttpResponse.json(proxies[group as string]);
  }),

  http.get('/api/configs', async () => {
    await delay(200);
    return HttpResponse.json(systemConfig)
  }),

  // 更新配置
  http.patch('/api/configs', async ({ request }) => {
    const updates = await request.json() as any;
    systemConfig = { ...systemConfig, ...updates };
    await delay(300);
    return HttpResponse.json(systemConfig);
  }),

  // 模拟实时流量
  http.get('/api/traffic', () => {
    return HttpResponse.json({
      up: Math.floor(Math.random() * 100 * 1024),
      down: Math.floor(Math.random() * 2 * 1024 * 1024),
    })
  }),

  // 清理 DNS
  http.post('/api/dns/flush', async () => {
    await delay(800);
    return new HttpResponse(null, { status: 204 });
  }),

  // 节点测速
  http.post('/api/nodes/test', async ({ request }: { request: Request }) => {
    const { name } = await request.json() as { name: string };
    await delay(1200);
    return HttpResponse.json({
      name,
      delay: Math.floor(Math.random() * 100) + 10
    })
  }),

  // 最近连接记录
  http.get('/api/connections', () => {
    return HttpResponse.json([
      { id: '1', domain: 'google.com', rule: 'Google', policy: '🚀 自动选择', speed: '24 KB/s' },
      { id: '2', domain: 'github.com', rule: 'GitHub', policy: '🚀 自动选择', speed: '12 KB/s' },
      { id: '3', domain: 'netflix.com', rule: 'Streaming', policy: '🎬 流媒体', speed: '2.4 MB/s' },
      { id: '4', domain: 'analytics.io', rule: 'MATCH', policy: '🐟 漏网之鱼', speed: '1 KB/s' },
    ])
  }),

  // 获取运行日志
  http.get('/api/logs', async () => {
    await delay(200);
    return HttpResponse.json(logs);
  })
]
