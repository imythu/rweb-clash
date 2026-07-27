import { useState } from 'react';
import { Save } from 'lucide-react';
import type { SystemConfig } from '@/lib/api';
import { Button } from '@/components/ui/button';
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from '@/components/ui/field';
import { Textarea } from '@/components/ui/textarea';
import { Spinner } from '@/components/ui/spinner';

type DnsPatch = Pick<
  SystemConfig,
  'dns_nameservers' | 'dns_fallback' | 'dns_fake_ip_filter' | 'dns_nameserver_policy' | 'dns_hosts'
>;

type DnsDraft = {
  nameservers: string;
  fallback: string;
  fakeIpFilter: string;
  policy: string;
  hosts: string;
};

function lines(values: string[]) {
  return values.join('\n');
}

function mapLines(values: Record<string, string[]>) {
  return Object.entries(values)
    .map(([key, entries]) => `${key} = ${entries.join(', ')}`)
    .join('\n');
}

function draftFromConfig(config: SystemConfig): DnsDraft {
  return {
    nameservers: lines(config.dns_nameservers),
    fallback: lines(config.dns_fallback),
    fakeIpFilter: lines(config.dns_fake_ip_filter),
    policy: mapLines(config.dns_nameserver_policy),
    hosts: mapLines(config.dns_hosts),
  };
}

function parseList(value: string) {
  return value
    .split(/\r?\n/)
    .map(entry => entry.trim())
    .filter(Boolean);
}

function parseMap(value: string, label: string) {
  const output: Record<string, string[]> = {};
  for (const [index, line] of value.split(/\r?\n/).entries()) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const delimiter = trimmed.indexOf('=');
    if (delimiter <= 0) throw new Error(`${label}第 ${index + 1} 行缺少 =`);
    const key = trimmed.slice(0, delimiter).trim();
    const entries = trimmed
      .slice(delimiter + 1)
      .split(',')
      .map(entry => entry.trim())
      .filter(Boolean);
    if (!key || entries.length === 0) throw new Error(`${label}第 ${index + 1} 行内容不完整`);
    output[key] = entries;
  }
  return output;
}

export function AdvancedDnsSettings({
  config,
  saving,
  onSave,
  onError,
}: {
  config: SystemConfig;
  saving: boolean;
  onSave: (patch: DnsPatch) => void | Promise<void>;
  onError: (message: string) => void;
}) {
  const [draft, setDraft] = useState(() => draftFromConfig(config));

  const save = () => {
    try {
      const patch: DnsPatch = {
        dns_nameservers: parseList(draft.nameservers),
        dns_fallback: parseList(draft.fallback),
        dns_fake_ip_filter: parseList(draft.fakeIpFilter),
        dns_nameserver_policy: parseMap(draft.policy, '域名策略'),
        dns_hosts: parseMap(draft.hosts, 'Hosts'),
      };
      void onSave(patch);
    } catch (error) {
      onError(error instanceof Error ? error.message : 'DNS 配置格式错误');
    }
  };

  return (
    <div className="mt-6 border-t pt-6">
      <div className="mb-5 flex flex-col justify-between gap-3 sm:flex-row sm:items-center">
        <div>
          <h4 className="text-sm font-semibold">高级 DNS</h4>
          <p className="mt-1 text-xs text-muted-foreground">每行一个值；策略与 Hosts 使用“匹配项 = 值1, 值2”。</p>
        </div>
        <Button type="button" size="sm" disabled={saving} onClick={save}>
          {saving ? <Spinner data-icon="inline-start" /> : <Save data-icon="inline-start" />}
          保存 DNS
        </Button>
      </div>
      <FieldGroup>
        <div className="grid grid-cols-1 gap-5 lg:grid-cols-2">
          <Field data-invalid={config.dns_enabled && parseList(draft.nameservers).length === 0}>
            <FieldLabel htmlFor="dns-nameservers">Nameserver</FieldLabel>
            <Textarea
              id="dns-nameservers"
              rows={5}
              aria-invalid={config.dns_enabled && parseList(draft.nameservers).length === 0}
              value={draft.nameservers}
              onChange={event => setDraft(current => ({ ...current, nameservers: event.target.value }))}
            />
            <FieldDescription>支持 UDP、TCP、DoT、DoH、QUIC 和 system。</FieldDescription>
          </Field>
          <Field>
            <FieldLabel htmlFor="dns-fallback">Fallback</FieldLabel>
            <Textarea id="dns-fallback" rows={5} value={draft.fallback} onChange={event => setDraft(current => ({ ...current, fallback: event.target.value }))} />
            <FieldDescription>留空则不生成 fallback。</FieldDescription>
          </Field>
          <Field>
            <FieldLabel htmlFor="dns-fake-filter">Fake-IP 过滤</FieldLabel>
            <Textarea id="dns-fake-filter" rows={6} value={draft.fakeIpFilter} onChange={event => setDraft(current => ({ ...current, fakeIpFilter: event.target.value }))} />
            <FieldDescription>支持域名、通配符、geosite 与 rule-set 形式。</FieldDescription>
          </Field>
          <Field>
            <FieldLabel htmlFor="dns-policy">域名策略</FieldLabel>
            <Textarea id="dns-policy" rows={6} value={draft.policy} onChange={event => setDraft(current => ({ ...current, policy: event.target.value }))} />
            <FieldDescription>例如：geosite:cn = https://doh.pub/dns-query</FieldDescription>
          </Field>
        </div>
        <Field>
          <FieldLabel htmlFor="dns-hosts">Hosts</FieldLabel>
          <Textarea id="dns-hosts" rows={5} value={draft.hosts} onChange={event => setDraft(current => ({ ...current, hosts: event.target.value }))} />
          <FieldDescription>例如：router.local = 192.168.1.1</FieldDescription>
        </Field>
      </FieldGroup>
    </div>
  );
}
