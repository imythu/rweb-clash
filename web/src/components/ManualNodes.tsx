import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Activity,
  Edit3,
  Plus,
  Save,
  Search,
  Server,
  Trash2,
} from 'lucide-react';
import { api, type ManualNode, type ManualNodeInput } from '@/lib/api';
import {
  fieldsForProtocol,
  MANUAL_NODE_PROTOCOL_ENTRIES,
  MANUAL_NODE_PROTOCOLS,
  type ManualNodeField,
} from '@/lib/manual-node-schema';
import { useToast } from './toast-context';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
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
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty';
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSeparator,
  FieldSet,
} from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Spinner } from '@/components/ui/spinner';
import { Switch } from '@/components/ui/switch';
import { Textarea } from '@/components/ui/textarea';

type DraftValue = unknown;

type KeyValueDraftRow = {
  name: string;
  value: string | string[];
};

type ObjectDraftRow = {
  values: Record<string, DraftValue>;
  preservedConfig: Record<string, unknown>;
};

type Draft = {
  name: string;
  type: string;
  values: Record<string, DraftValue>;
  preservedConfig: Record<string, unknown>;
  unknownPaths: string[];
};

const PROTOCOL_CATEGORIES = ['常用协议', 'QUIC / VPN', '特殊出站'] as const;
const RESERVED_CONFIG_KEYS = ['name', 'type'];
const UNSET_SELECT_VALUE = '__manual-node-unset__';

function newDraft(type = 'ss'): Draft {
  return {
    name: '',
    type,
    values: {
      server: '',
      port: type === 'mieru' ? '' : '443',
    },
    preservedConfig: {},
    unknownPaths: [],
  };
}

function cloneConfig<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function getNested(source: Record<string, unknown>, path: string): unknown {
  return path.split('.').reduce<unknown>((value, key) => {
    if (!value || typeof value !== 'object' || Array.isArray(value)) return undefined;
    return (value as Record<string, unknown>)[key];
  }, source);
}

function setNested(target: Record<string, unknown>, path: string, value: unknown) {
  const keys = path.split('.');
  let cursor = target;
  for (const key of keys.slice(0, -1)) {
    const nested = cursor[key];
    if (!nested || typeof nested !== 'object' || Array.isArray(nested)) cursor[key] = {};
    cursor = cursor[key] as Record<string, unknown>;
  }
  cursor[keys[keys.length - 1]] = value;
}

function deleteNested(target: Record<string, unknown>, path: string) {
  const keys = path.split('.');
  const parents: Array<[Record<string, unknown>, string]> = [];
  let cursor = target;
  for (const key of keys.slice(0, -1)) {
    const nested = cursor[key];
    if (!nested || typeof nested !== 'object' || Array.isArray(nested)) return;
    parents.push([cursor, key]);
    cursor = nested as Record<string, unknown>;
  }
  delete cursor[keys[keys.length - 1]];
  for (const [parent, key] of parents.reverse()) {
    const value = parent[key];
    if (value && typeof value === 'object' && !Array.isArray(value) && Object.keys(value).length === 0) {
      delete parent[key];
    }
  }
}

function fieldKind(field: ManualNodeField) {
  return field.kind ?? 'text';
}

function scalarDraftValue(value: unknown): DraftValue {
  if (typeof value === 'boolean') return value;
  return value === undefined || value === null ? '' : String(value);
}

function objectDraftRow(
  value: Record<string, unknown>,
  itemFields: ManualNodeField[],
): ObjectDraftRow {
  const preservedConfig = cloneConfig(value);
  const values: Record<string, DraftValue> = {};
  for (const itemField of itemFields) {
    values[itemField.key] = editValue(itemField, getNested(value, itemField.key));
    deleteNested(preservedConfig, itemField.key);
  }
  return { values, preservedConfig };
}

function editValue(field: ManualNodeField, value: unknown): DraftValue {
  switch (fieldKind(field)) {
    case 'boolean':
      return typeof value === 'boolean' ? value : undefined;
    case 'string-list':
    case 'number-list':
      if (Array.isArray(value)) return value.map(String);
      return value === undefined || value === null || value === '' ? [] : [String(value)];
    case 'key-value-list':
      if (!value || typeof value !== 'object' || Array.isArray(value)) return [];
      return Object.entries(value).map(([name, item]): KeyValueDraftRow => ({
        name,
        value: field.valueKind === 'string-list'
          ? (Array.isArray(item) ? item.map(String) : [String(item ?? '')])
          : String(item ?? ''),
      }));
    case 'object-list':
      if (!Array.isArray(value)) return [];
      return value
        .filter((item): item is Record<string, unknown> => Boolean(item) && typeof item === 'object' && !Array.isArray(item))
        .map(item => objectDraftRow(item, field.itemFields ?? []));
    default:
      return scalarDraftValue(value);
  }
}

function protocolFields(type: string) {
  return fieldsForProtocol(type);
}

function conditionValueMatches(field: ManualNodeField, values: Record<string, DraftValue>) {
  const condition = field.showWhen;
  if (!condition) return true;
  const value = values[condition.key];
  if (condition.equals !== undefined) {
    return value === condition.equals || String(value ?? '') === String(condition.equals);
  }
  if (condition.oneOf) {
    return condition.oneOf.some(option => value === option || String(value ?? '') === String(option));
  }
  const truthy = Array.isArray(value)
    ? value.length > 0
    : Boolean(value && (typeof value !== 'object' || Object.keys(value).length > 0));
  if (condition.truthy !== undefined) return truthy === condition.truthy;
  return truthy;
}

function visibleFields(fields: ManualNodeField[], values: Record<string, DraftValue>) {
  const byKey = new Map(fields.map(field => [field.key, field]));
  const isVisible = (field: ManualNodeField, visiting: Set<string>): boolean => {
    const condition = field.showWhen;
    if (!condition) return true;
    if (visiting.has(field.key)) return false;
    const controller = byKey.get(condition.key);
    if (controller) {
      const next = new Set(visiting);
      next.add(field.key);
      if (!isVisible(controller, next)) return false;
    }
    return conditionValueMatches(field, values);
  };
  return fields.filter(field => isVisible(field, new Set()));
}

function nonEmptyStrings(value: DraftValue): string[] {
  if (!Array.isArray(value)) return [];
  return value.map(String).map(item => item.trim()).filter(Boolean);
}

function isObjectDraftRow(value: unknown): value is ObjectDraftRow {
  return value !== null
    && typeof value === 'object'
    && !Array.isArray(value)
    && 'values' in value
    && 'preservedConfig' in value;
}

function normalizedFieldValue(field: ManualNodeField, value: DraftValue): unknown {
  switch (fieldKind(field)) {
    case 'boolean':
      return typeof value === 'boolean' ? value : undefined;
    case 'number': {
      if (value === undefined || String(value).trim() === '') return undefined;
      const parsed = Number(value);
      return Number.isFinite(parsed) ? parsed : undefined;
    }
    case 'string-list': {
      const items = nonEmptyStrings(value);
      return items.length > 0 ? items : undefined;
    }
    case 'number-list': {
      const items = nonEmptyStrings(value).map(Number).filter(Number.isFinite);
      return items.length > 0 ? items : undefined;
    }
    case 'key-value-list': {
      if (!Array.isArray(value)) return undefined;
      const mapped: Record<string, unknown> = {};
      for (const rawRow of value) {
        const row = rawRow as KeyValueDraftRow;
        const name = String(row?.name ?? '').trim();
        if (!name) continue;
        if (field.valueKind === 'string-list') {
          const items = nonEmptyStrings(row.value);
          if (items.length > 0) mapped[name] = items;
        } else {
          const item = String(row?.value ?? '').trim();
          if (item) mapped[name] = item;
        }
      }
      return Object.keys(mapped).length > 0 ? mapped : undefined;
    }
    case 'object-list': {
      if (!Array.isArray(value)) return undefined;
      const itemFields = field.itemFields ?? [];
      const items = value.filter(isObjectDraftRow).map(row => {
        const item = cloneConfig(row.preservedConfig);
        for (const itemField of itemFields) deleteNested(item, itemField.key);
        for (const itemField of visibleFields(itemFields, row.values)) {
          const normalized = normalizedFieldValue(itemField, row.values[itemField.key]);
          if (normalized !== undefined) setNested(item, itemField.key, normalized);
        }
        return item;
      }).filter(item => Object.keys(item).length > 0);
      return items.length > 0 ? items : undefined;
    }
    case 'select': {
      if (value === undefined || value === null || String(value).trim() === '') return undefined;
      if (field.valueType === 'number') {
        const parsed = Number(value);
        return Number.isFinite(parsed) ? parsed : undefined;
      }
      return String(value).trim();
    }
    default: {
      if (value === undefined || value === null) return undefined;
      const normalized = String(value).trim();
      return normalized || undefined;
    }
  }
}

function configLeafPaths(value: unknown, prefix = ''): string[] {
  if (Array.isArray(value)) {
    if (value.length === 0) return prefix ? [prefix] : [];
    return value.flatMap((item, index) => configLeafPaths(item, `${prefix}[${index}]`));
  }
  if (value && typeof value === 'object') {
    const entries = Object.entries(value);
    if (entries.length === 0) return prefix ? [prefix] : [];
    return entries.flatMap(([key, item]) => configLeafPaths(item, prefix ? `${prefix}.${key}` : key));
  }
  return prefix ? [prefix] : [];
}

function unknownPathsForRecord(
  config: Record<string, unknown>,
  fields: ManualNodeField[],
  prefix = '',
  reservedKeys: string[] = [],
) {
  const remainder = cloneConfig(config);
  const nestedPaths: string[] = [];
  for (const key of reservedKeys) deleteNested(remainder, key);
  for (const field of fields) {
    if (field.kind === 'object-list') {
      const value = getNested(config, field.key);
      if (Array.isArray(value)) {
        value.forEach((item, index) => {
          if (item && typeof item === 'object' && !Array.isArray(item)) {
            const itemPrefix = prefix
              ? `${prefix}.${field.key}[${index}]`
              : `${field.key}[${index}]`;
            nestedPaths.push(...unknownPathsForRecord(
              item as Record<string, unknown>,
              field.itemFields ?? [],
              itemPrefix,
            ));
          }
        });
      }
    }
    deleteNested(remainder, field.key);
  }
  return [...nestedPaths, ...configLeafPaths(remainder, prefix)];
}

function unknownConfigPaths(config: Record<string, unknown>, fields: ManualNodeField[]) {
  return unknownPathsForRecord(config, fields, '', RESERVED_CONFIG_KEYS);
}

function validateFields(
  fields: ManualNodeField[],
  values: Record<string, DraftValue>,
  invalid: Set<string>,
  prefix = '',
) {
  for (const field of visibleFields(fields, values)) {
    const key = prefix ? `${prefix}.${field.key}` : field.key;
    const value = values[field.key];
    const normalized = normalizedFieldValue(field, value);
    if (field.required && normalized === undefined) invalid.add(key);

    if (field.kind === 'number' && value !== undefined && String(value).trim()) {
      const number = Number(value);
      if (!Number.isFinite(number)
        || (field.min !== undefined && number < field.min)
        || (field.max !== undefined && number > field.max)) {
        invalid.add(key);
      }
    }

    if (field.kind === 'number-list' && Array.isArray(value)) {
      if (value.some(item => {
        if (!String(item).trim()) return false;
        const number = Number(item);
        return !Number.isFinite(number)
          || (field.min !== undefined && number < field.min)
          || (field.max !== undefined && number > field.max);
      })) invalid.add(key);
    }

    if ((field.kind === 'string-list'
      || field.kind === 'number-list'
      || field.kind === 'key-value-list'
      || field.kind === 'object-list')
      && Array.isArray(value)) {
      const count = field.kind === 'string-list' || field.kind === 'number-list'
        ? nonEmptyStrings(value).length
        : value.length;
      if ((field.minItems !== undefined && count < field.minItems)
        || (field.maxItems !== undefined && count > field.maxItems)) {
        invalid.add(key);
      }
    }

    if (field.kind === 'key-value-list' && Array.isArray(value)) {
      const names = new Set<string>();
      for (const rawRow of value) {
        const row = rawRow as KeyValueDraftRow;
        const name = String(row?.name ?? '').trim();
        const hasValue = field.valueKind === 'string-list'
          ? nonEmptyStrings(row?.value).length > 0
          : Boolean(String(row?.value ?? '').trim());
        if ((!name && hasValue) || (name && !hasValue) || (name && names.has(name))) invalid.add(key);
        if (name) names.add(name);
      }
    }

    if (field.kind === 'object-list' && Array.isArray(value)) {
      const invalidCount = invalid.size;
      value.filter(isObjectDraftRow).forEach((row, index) => {
        validateFields(field.itemFields ?? [], row.values, invalid, `${key}.${index}`);
      });
      if (invalid.size > invalidCount) invalid.add(key);
    }
  }
}

function groupedFields(fields: ManualNodeField[], fallbackSection: string) {
  const groups = new Map<string, ManualNodeField[]>();
  for (const field of fields) {
    const section = field.section ?? fallbackSection;
    groups.set(section, [...(groups.get(section) ?? []), field]);
  }
  return [...groups.entries()];
}

function nodeAddress(node: ManualNode) {
  const firstPeer = Array.isArray(node.config.peers)
    && node.config.peers[0]
    && typeof node.config.peers[0] === 'object'
    && !Array.isArray(node.config.peers[0])
    ? node.config.peers[0] as Record<string, unknown>
    : undefined;
  const server = String(firstPeer?.server ?? node.config.server ?? '').trim();
  const port = String(firstPeer?.port ?? node.config.port ?? '').trim();
  return server ? `${server}${port ? `:${port}` : ''}` : '本地 / 虚拟出站';
}

function latencyLabel(latency: number) {
  if (latency > 0) return `${latency} ms`;
  if (latency === 0) return '超时';
  return '未测速';
}

function fieldId(key: string) {
  return `manual-node-${key.replace(/[^a-zA-Z0-9_-]/g, '-')}`;
}

function listValue(value: DraftValue) {
  return Array.isArray(value) ? value.map(String) : [];
}

function ListFieldControl({
  field,
  value,
  id,
  invalid,
  onChange,
}: {
  field: ManualNodeField;
  value: DraftValue;
  id: string;
  invalid: boolean;
  onChange: (value: DraftValue) => void;
}) {
  const items = listValue(value);
  return (
    <Field className="sm:col-span-2" data-invalid={invalid || undefined}>
      <FieldLabel htmlFor={`${id}-0`}>{field.label}{field.required ? ' *' : ''}</FieldLabel>
      {field.description && <FieldDescription>{field.description}</FieldDescription>}
      <FieldGroup className="gap-2">
        {items.map((item, index) => (
          <Field key={`${id}-${index}`} orientation="horizontal">
            <FieldLabel htmlFor={`${id}-${index}`} className="sr-only">{field.label} {index + 1}</FieldLabel>
            <Input
              id={`${id}-${index}`}
              type={field.kind === 'number-list' ? 'number' : 'text'}
              min={field.min}
              max={field.max}
              step={field.step}
              value={item}
              placeholder={field.placeholder}
              aria-invalid={invalid || undefined}
              onChange={event => {
                const next = [...items];
                next[index] = event.target.value;
                onChange(next);
              }}
            />
            <Button
              type="button"
              variant="ghost"
              size="icon"
              title={`删除${field.label}`}
              disabled={field.minItems !== undefined && items.length <= field.minItems}
              onClick={() => onChange(items.filter((_, itemIndex) => itemIndex !== index))}
            >
              <Trash2 />
              <span className="sr-only">删除{field.label}第 {index + 1} 项</span>
            </Button>
          </Field>
        ))}
      </FieldGroup>
      <Button
        type="button"
        variant="outline"
        size="sm"
        disabled={field.maxItems !== undefined && items.length >= field.maxItems}
        onClick={() => onChange([...items, ''])}
      >
        <Plus data-icon="inline-start" />
        添加{field.label}
      </Button>
    </Field>
  );
}

function KeyValueFieldControl({
  field,
  value,
  id,
  invalid,
  onChange,
}: {
  field: ManualNodeField;
  value: DraftValue;
  id: string;
  invalid: boolean;
  onChange: (value: DraftValue) => void;
}) {
  const rows = Array.isArray(value) ? value as KeyValueDraftRow[] : [];
  return (
    <Field className="sm:col-span-2" data-invalid={invalid || undefined}>
      <FieldLabel htmlFor={`${id}-name-0`}>{field.label}{field.required ? ' *' : ''}</FieldLabel>
      {field.description && <FieldDescription>{field.description}</FieldDescription>}
      <FieldGroup className="gap-3">
        {rows.map((row, index) => (
          <FieldGroup key={`${id}-${index}`} className="grid grid-cols-1 items-start gap-2 sm:grid-cols-[minmax(7rem,0.7fr)_minmax(0,1.3fr)_2.5rem]">
            <Field>
              <FieldLabel htmlFor={`${id}-name-${index}`} className="sr-only">{field.label}键名</FieldLabel>
              <Input
                id={`${id}-name-${index}`}
                value={row.name}
                placeholder="名称"
                aria-invalid={invalid || undefined}
                onChange={event => {
                  const next = [...rows];
                  next[index] = { ...row, name: event.target.value };
                  onChange(next);
                }}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor={`${id}-value-${index}`} className="sr-only">{field.label}值</FieldLabel>
              {field.valueKind === 'string-list' ? (
                <Textarea
                  id={`${id}-value-${index}`}
                  rows={2}
                  value={listValue(row.value).join('\n')}
                  placeholder="每行一个值"
                  aria-invalid={invalid || undefined}
                  onChange={event => {
                    const next = [...rows];
                    next[index] = { ...row, value: event.target.value.split('\n') };
                    onChange(next);
                  }}
                />
              ) : (
                <Input
                  id={`${id}-value-${index}`}
                  value={String(row.value ?? '')}
                  placeholder="值"
                  aria-invalid={invalid || undefined}
                  onChange={event => {
                    const next = [...rows];
                    next[index] = { ...row, value: event.target.value };
                    onChange(next);
                  }}
                />
              )}
            </Field>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="justify-self-end"
              title={`删除${field.label}`}
              disabled={field.minItems !== undefined && rows.length <= field.minItems}
              onClick={() => onChange(rows.filter((_, rowIndex) => rowIndex !== index))}
            >
              <Trash2 />
              <span className="sr-only">删除{field.label}第 {index + 1} 项</span>
            </Button>
          </FieldGroup>
        ))}
      </FieldGroup>
      <Button
        type="button"
        variant="outline"
        size="sm"
        disabled={field.maxItems !== undefined && rows.length >= field.maxItems}
        onClick={() => onChange([...rows, { name: '', value: field.valueKind === 'string-list' ? [] : '' }])}
      >
        <Plus data-icon="inline-start" />
        添加{field.label}
      </Button>
    </Field>
  );
}

function ObjectListFieldControl({
  field,
  value,
  id,
  invalid,
  onChange,
}: {
  field: ManualNodeField;
  value: DraftValue;
  id: string;
  invalid: boolean;
  onChange: (value: DraftValue) => void;
}) {
  const rows = Array.isArray(value) ? value.filter(isObjectDraftRow) : [];
  const itemFields = field.itemFields ?? [];
  return (
    <Field className="sm:col-span-2" data-invalid={invalid || undefined}>
      <FieldLabel>{field.label}{field.required ? ' *' : ''}</FieldLabel>
      {field.description && <FieldDescription>{field.description}</FieldDescription>}
      <FieldGroup className="gap-4">
        {rows.map((row, index) => (
          <FieldSet key={`${id}-${index}`} className="gap-4 rounded-md border p-4">
            <FieldLegend variant="label">{field.label} {index + 1}</FieldLegend>
            <FieldGroup className="grid grid-cols-1 gap-4 sm:grid-cols-2">
              {visibleFields(itemFields, row.values).map(itemField => (
                <FieldControl
                  key={itemField.key}
                  field={itemField}
                  value={row.values[itemField.key]}
                  invalid={false}
                  idPrefix={`${id}-${index}`}
                  onChange={itemValue => {
                    const next = [...rows];
                    next[index] = {
                      ...row,
                      values: { ...row.values, [itemField.key]: itemValue },
                    };
                    onChange(next);
                  }}
                />
              ))}
            </FieldGroup>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              disabled={field.minItems !== undefined && rows.length <= field.minItems}
              onClick={() => onChange(rows.filter((_, rowIndex) => rowIndex !== index))}
            >
              <Trash2 data-icon="inline-start" />
              删除{field.label} {index + 1}
            </Button>
          </FieldSet>
        ))}
      </FieldGroup>
      <Button
        type="button"
        variant="outline"
        size="sm"
        disabled={field.maxItems !== undefined && rows.length >= field.maxItems}
        onClick={() => onChange([
          ...rows,
          objectDraftRow({}, itemFields),
        ])}
      >
        <Plus data-icon="inline-start" />
        添加{field.label}
      </Button>
    </Field>
  );
}

function FieldControl({
  field,
  value,
  invalid,
  idPrefix,
  onChange,
}: {
  field: ManualNodeField;
  value: DraftValue | undefined;
  invalid: boolean;
  idPrefix?: string;
  onChange: (value: DraftValue) => void;
}) {
  const id = idPrefix ? `${idPrefix}-${fieldId(field.key)}` : fieldId(field.key);
  if (field.kind === 'string-list' || field.kind === 'number-list') {
    return <ListFieldControl field={field} value={value} id={id} invalid={invalid} onChange={onChange} />;
  }
  if (field.kind === 'key-value-list') {
    return <KeyValueFieldControl field={field} value={value} id={id} invalid={invalid} onChange={onChange} />;
  }
  if (field.kind === 'object-list') {
    return <ObjectListFieldControl field={field} value={value} id={id} invalid={invalid} onChange={onChange} />;
  }
  if (field.kind === 'boolean') {
    return (
      <Field orientation="horizontal" data-invalid={invalid || undefined}>
        <FieldContent>
          <FieldLabel htmlFor={id}>{field.label}</FieldLabel>
          {field.description && <FieldDescription>{field.description}</FieldDescription>}
        </FieldContent>
        <Switch
          id={id}
          checked={Boolean(value)}
          aria-invalid={invalid || undefined}
          onCheckedChange={onChange}
        />
      </Field>
    );
  }

  return (
    <Field
      className={field.kind === 'textarea' ? 'sm:col-span-2' : undefined}
      data-invalid={invalid || undefined}
    >
      <FieldLabel htmlFor={id}>{field.label}{field.required ? ' *' : ''}</FieldLabel>
      {field.kind === 'select' ? (
        <Select
          value={String(value ?? '') || UNSET_SELECT_VALUE}
          onValueChange={next => onChange(next === UNSET_SELECT_VALUE ? '' : next)}
        >
          <SelectTrigger id={id} aria-invalid={invalid || undefined}>
            <SelectValue placeholder="请选择" />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              {!field.required && <SelectItem value={UNSET_SELECT_VALUE}>未设置</SelectItem>}
              {field.options?.map(option => (
                <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>
              ))}
            </SelectGroup>
          </SelectContent>
        </Select>
      ) : field.kind === 'textarea' ? (
        <Textarea
          id={id}
          rows={6}
          value={String(value ?? '')}
          placeholder={field.placeholder}
          aria-invalid={invalid || undefined}
          onChange={event => onChange(event.target.value)}
        />
      ) : (
        <Input
          id={id}
          type={field.kind === 'password' ? 'password' : field.kind === 'number' ? 'number' : 'text'}
          min={field.min}
          max={field.max}
          step={field.step}
          value={String(value ?? '')}
          placeholder={field.placeholder}
          aria-invalid={invalid || undefined}
          onChange={event => onChange(event.target.value)}
        />
      )}
      {field.description && <FieldDescription>{field.description}</FieldDescription>}
    </Field>
  );
}

export function ManualNodes() {
  const { toast } = useToast();
  const [nodes, setNodes] = useState<ManualNode[]>([]);
  const [loading, setLoading] = useState(true);
  const [search, setSearch] = useState('');
  const [draft, setDraft] = useState<Draft>(() => newDraft());
  const [editingName, setEditingName] = useState<string | null>(null);
  const [editorOpen, setEditorOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<ManualNode | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [testingName, setTestingName] = useState<string | null>(null);
  const [invalidKeys, setInvalidKeys] = useState<Set<string>>(() => new Set());
  const mutationInFlight = useRef(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setNodes(await api.listManualNodes());
    } catch {
      toast('手动节点加载失败', 'error');
    } finally {
      setLoading(false);
    }
  }, [toast]);

  useEffect(() => {
    queueMicrotask(() => void load());
  }, [load]);

  const filteredNodes = useMemo(() => {
    const query = search.trim().toLowerCase();
    if (!query) return nodes;
    return nodes.filter(node => [
      node.displayName,
      node.type,
      MANUAL_NODE_PROTOCOLS[node.type]?.label,
      nodeAddress(node),
    ].some(value => String(value ?? '').toLowerCase().includes(query)));
  }, [nodes, search]);

  const definition = MANUAL_NODE_PROTOCOLS[draft.type];
  const fields = useMemo(() => protocolFields(draft.type), [draft.type]);
  const shownFields = visibleFields(fields, draft.values);
  const fieldGroups = groupedFields(shownFields, definition.label);

  const openCreate = () => {
    setEditingName(null);
    setDraft(newDraft());
    setInvalidKeys(new Set());
    setEditorOpen(true);
  };

  const openEdit = (node: ManualNode) => {
    const type = node.type in MANUAL_NODE_PROTOCOLS ? node.type : 'ss';
    const config = node.config;
    const nodeFields = protocolFields(type);
    const values: Record<string, DraftValue> = {};
    for (const field of nodeFields) values[field.key] = editValue(field, getNested(config, field.key));

    setEditingName(node.name);
    setDraft({
      name: node.displayName,
      type,
      values,
      preservedConfig: cloneConfig(config),
      unknownPaths: unknownConfigPaths(config, nodeFields),
    });
    setInvalidKeys(new Set());
    setEditorOpen(true);
  };

  const updateValue = (key: string, value: DraftValue) => {
    setDraft(current => ({ ...current, values: { ...current.values, [key]: value } }));
    setInvalidKeys(current => {
      if (![...current].some(invalidKey => invalidKey === key || invalidKey.startsWith(`${key}.`))) return current;
      const next = new Set(current);
      for (const invalidKey of next) {
        if (invalidKey === key || invalidKey.startsWith(`${key}.`)) next.delete(invalidKey);
      }
      return next;
    });
  };

  const changeProtocol = (type: string) => {
    setDraft(current => ({
      ...newDraft(type),
      name: current.name,
      values: {
        server: current.values.server ?? '',
        port: type === 'mieru' ? '' : current.values.port || '443',
      },
    }));
    setInvalidKeys(new Set());
  };

  const validate = () => {
    const invalid = new Set<string>();
    const textValue = (key: string) => String(draft.values[key] ?? '').trim();
    if (!draft.name.trim()) invalid.add('name');
    validateFields(fields, draft.values, invalid);
    if (draft.type === 'tuic') {
      const token = textValue('token');
      const uuid = textValue('uuid');
      const password = textValue('password');
      if ((!token && (!uuid || !password)) || (token && (uuid || password))) {
        invalid.add('token');
        invalid.add('uuid');
        invalid.add('password');
      }
    }
    if (draft.type === 'rematch'
      && !textValue('target-rematch-name')
      && !textValue('target-sub-rule')) {
      invalid.add('target-rematch-name');
      invalid.add('target-sub-rule');
    }
    if (draft.type === 'openvpn') {
      const username = textValue('username');
      const password = textValue('password');
      const certificate = textValue('cert');
      const privateKey = textValue('key');
      if (!(username && password) && !(certificate && privateKey)) {
        for (const key of ['username', 'password', 'cert', 'key']) invalid.add(key);
      }
      if (Boolean(username) !== Boolean(password)) {
        invalid.add('username');
        invalid.add('password');
      }
      if (Boolean(certificate) !== Boolean(privateKey)) {
        invalid.add('cert');
        invalid.add('key');
      }
      const tlsKeys = ['tls-auth', 'tls-crypt', 'tls-crypt-v2'].filter(key => textValue(key));
      if (tlsKeys.length > 1) tlsKeys.forEach(key => invalid.add(key));
    }
    if (draft.type === 'sudoku') {
      const paddingMin = textValue('padding-min');
      const paddingMax = textValue('padding-max');
      if (paddingMin && paddingMax && Number(paddingMax) < Number(paddingMin)) {
        invalid.add('padding-min');
        invalid.add('padding-max');
      }
    }
    setInvalidKeys(invalid);
    return invalid.size === 0;
  };

  const save = async () => {
    if (mutationInFlight.current || !validate()) {
      if (!mutationInFlight.current) toast('请检查标记的必填字段', 'error');
      return;
    }

    const config = cloneConfig(draft.preservedConfig);
    delete config.name;
    config.type = draft.type;
    for (const field of fields) {
      deleteNested(config, field.key);
    }
    for (const field of shownFields) {
      const normalized = normalizedFieldValue(field, draft.values[field.key]);
      if (normalized !== undefined) setNested(config, field.key, normalized);
    }

    const input: ManualNodeInput = { name: draft.name.trim(), config };
    mutationInFlight.current = true;
    setSaving(true);
    try {
      const next = editingName
        ? await api.updateManualNode(editingName, input)
        : await api.createManualNode(input);
      setNodes(next);
      setEditorOpen(false);
      setEditingName(null);
      setDraft(newDraft());
      toast(editingName ? '手动节点已更新' : '手动节点已添加', 'success');
    } catch {
      toast('节点保存失败，请检查协议参数或节点名称', 'error');
    } finally {
      mutationInFlight.current = false;
      setSaving(false);
    }
  };

  const remove = async () => {
    if (!deleteTarget || mutationInFlight.current) return;
    mutationInFlight.current = true;
    setDeleting(true);
    try {
      await api.deleteManualNode(deleteTarget.name);
      setNodes(current => current.filter(node => node.name !== deleteTarget.name));
      setDeleteTarget(null);
      toast('手动节点已删除', 'success');
    } catch {
      toast('节点正在被分组或路由引用，无法删除', 'error');
    } finally {
      mutationInFlight.current = false;
      setDeleting(false);
    }
  };

  const testNode = async (node: ManualNode) => {
    if (testingName) return;
    setTestingName(node.name);
    try {
      const result = await api.testNode(node.name);
      setNodes(current => current.map(item => item.name === node.name ? { ...item, latency: result.delay } : item));
      toast(result.delay > 0 ? '节点测速完成' : '节点测速超时', result.delay > 0 ? 'success' : 'info');
    } catch {
      toast('节点测速失败', 'error');
    } finally {
      setTestingName(null);
    }
  };

  return (
    <div className="mx-auto flex w-full max-w-6xl flex-col gap-6 pb-8">
      <header className="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
        <div className="flex flex-col gap-1">
          <h2 className="text-2xl font-black tracking-tight">手动节点</h2>
          <p className="text-sm text-muted-foreground">自建节点与订阅节点共享分组和路由。</p>
        </div>
        <Button onClick={openCreate}>
          <Plus data-icon="inline-start" />
          新增节点
        </Button>
      </header>

      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="w-full sm:max-w-sm">
          <Field>
            <FieldLabel htmlFor="manual-node-search" className="sr-only">搜索手动节点</FieldLabel>
            <Input
              id="manual-node-search"
              value={search}
              onChange={event => setSearch(event.target.value)}
              placeholder="搜索名称、协议或地址"
            />
          </Field>
        </div>
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <Search />
          <span>{filteredNodes.length} / {nodes.length} 个节点</span>
        </div>
      </div>

      {loading ? (
        <div className="flex min-h-72 items-center justify-center"><Spinner /></div>
      ) : filteredNodes.length === 0 ? (
        <Empty className="min-h-72 border">
          <EmptyHeader>
            <EmptyMedia variant="icon"><Server /></EmptyMedia>
            <EmptyTitle>{nodes.length === 0 ? '暂无手动节点' : '没有匹配的节点'}</EmptyTitle>
            <EmptyDescription>{nodes.length === 0 ? '从一个自建节点开始。' : '尝试调整搜索内容。'}</EmptyDescription>
          </EmptyHeader>
          {nodes.length === 0 && (
            <EmptyContent>
              <Button onClick={openCreate}>
                <Plus data-icon="inline-start" />
                新增节点
              </Button>
            </EmptyContent>
          )}
        </Empty>
      ) : (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
          {filteredNodes.map(node => (
            <article key={node.name} className="flex min-w-0 flex-col gap-4 rounded-lg border bg-card p-4 shadow-sm">
              <div className="flex min-w-0 items-start gap-3">
                <div className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-muted text-foreground">
                  <Server className="size-5" />
                </div>
                <div className="min-w-0 flex-1">
                  <h3 className="break-all text-sm font-semibold">{node.displayName}</h3>
                  <p className="mt-1 truncate font-mono text-xs text-muted-foreground" title={nodeAddress(node)}>{nodeAddress(node)}</p>
                </div>
                <Badge variant="outline">{MANUAL_NODE_PROTOCOLS[node.type]?.label ?? node.type}</Badge>
              </div>
              <div className="flex items-center justify-between gap-3 border-t pt-3">
                <Badge variant={node.latency > 0 ? 'secondary' : 'outline'}>{latencyLabel(node.latency)}</Badge>
                <div className="flex items-center gap-1">
                  <Button
                    variant="ghost"
                    size="icon"
                    title="测试节点延迟"
                    disabled={testingName !== null}
                    onClick={() => void testNode(node)}
                  >
                    {testingName === node.name ? <Spinner /> : <Activity />}
                    <span className="sr-only">测试节点延迟</span>
                  </Button>
                  <Button variant="ghost" size="icon" title="编辑节点" onClick={() => openEdit(node)}>
                    <Edit3 />
                    <span className="sr-only">编辑节点</span>
                  </Button>
                  <Button variant="ghost" size="icon" title="删除节点" onClick={() => setDeleteTarget(node)}>
                    <Trash2 />
                    <span className="sr-only">删除节点</span>
                  </Button>
                </div>
              </div>
            </article>
          ))}
        </div>
      )}

      <Dialog open={editorOpen} onOpenChange={open => { if (!saving) setEditorOpen(open); }}>
        <DialogContent className="flex max-h-[92vh] max-w-3xl flex-col overflow-hidden p-0">
          <DialogHeader className="border-b px-6 pb-4 pt-6">
            <DialogTitle>{editingName ? '编辑手动节点' : '新增手动节点'}</DialogTitle>
            <DialogDescription>配置项随 Mihomo 代理类型切换。</DialogDescription>
          </DialogHeader>

          <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">
            <FieldGroup>
              <FieldGroup className="grid grid-cols-1 gap-4 sm:grid-cols-2">
                <Field data-invalid={invalidKeys.has('name') || undefined}>
                  <FieldLabel htmlFor="manual-node-name">节点名称 *</FieldLabel>
                  <Input
                    id="manual-node-name"
                    value={draft.name}
                    disabled={editingName !== null}
                    aria-invalid={invalidKeys.has('name') || undefined}
                    onChange={event => {
                      setDraft(current => ({ ...current, name: event.target.value }));
                      setInvalidKeys(current => {
                        const next = new Set(current);
                        next.delete('name');
                        return next;
                      });
                    }}
                  />
                </Field>
                <Field>
                  <FieldLabel htmlFor="manual-node-type">代理类型</FieldLabel>
                  <Select value={draft.type} onValueChange={changeProtocol}>
                    <SelectTrigger id="manual-node-type"><SelectValue /></SelectTrigger>
                    <SelectContent>
                      {PROTOCOL_CATEGORIES.map(category => (
                        <SelectGroup key={category}>
                          <SelectLabel>{category}</SelectLabel>
                          {MANUAL_NODE_PROTOCOL_ENTRIES
                            .filter(([, protocol]) => protocol.category === category)
                            .map(([value, protocol]) => (
                              <SelectItem key={value} value={value}>{protocol.label} ({value})</SelectItem>
                            ))}
                        </SelectGroup>
                      ))}
                    </SelectContent>
                  </Select>
                </Field>
              </FieldGroup>

              {draft.unknownPaths.length > 0 && (
                <Alert>
                  <AlertTitle>存在未识别的旧参数</AlertTitle>
                  <AlertDescription>
                    保存时会原样保留：{draft.unknownPaths.slice(0, 8).join('、')}
                    {draft.unknownPaths.length > 8 ? ` 等 ${draft.unknownPaths.length} 项` : ''}
                  </AlertDescription>
                </Alert>
              )}

              {fieldGroups.length > 0 ? fieldGroups.map(([section, sectionFields]) => (
                <Fragment key={section}>
                  <FieldSeparator>{section}</FieldSeparator>
                  <FieldGroup className="grid grid-cols-1 gap-5 sm:grid-cols-2">
                    {sectionFields.map(field => (
                      <FieldControl
                        key={field.key}
                        field={field}
                        value={draft.values[field.key]}
                        invalid={invalidKeys.has(field.key)}
                        onChange={value => updateValue(field.key, value)}
                      />
                    ))}
                  </FieldGroup>
                </Fragment>
              )) : (
                <>
                  <FieldSeparator>{definition.label}</FieldSeparator>
                  <Alert>
                    <Server />
                    <AlertTitle>无需额外参数</AlertTitle>
                    <AlertDescription>该出站只需要名称和类型。</AlertDescription>
                  </Alert>
                </>
              )}
            </FieldGroup>
          </div>

          <DialogFooter className="border-t px-6 py-4">
            <Button variant="outline" disabled={saving} onClick={() => setEditorOpen(false)}>取消</Button>
            <Button disabled={saving} onClick={() => void save()}>
              {saving ? <Spinner data-icon="inline-start" /> : <Save data-icon="inline-start" />}
              {saving ? '保存中' : '保存节点'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <AlertDialog open={deleteTarget !== null} onOpenChange={open => { if (!open && !deleting) setDeleteTarget(null); }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>删除手动节点？</AlertDialogTitle>
            <AlertDialogDescription>{deleteTarget?.displayName} 将从所有自动分组中移除。</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={deleting}>取消</AlertDialogCancel>
            <AlertDialogAction disabled={deleting} onClick={() => void remove()}>
              {deleting && <Spinner data-icon="inline-start" />}
              {deleting ? '正在删除' : '确认删除'}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
