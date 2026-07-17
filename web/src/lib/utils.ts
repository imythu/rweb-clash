import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"

export const SUB_DELIMITER = '^_^';

let fallbackIdSequence = 0;

export function createId() {
  const cryptoApi = globalThis.crypto;

  if (typeof cryptoApi?.randomUUID === 'function') {
    return cryptoApi.randomUUID();
  }

  if (typeof cryptoApi?.getRandomValues === 'function') {
    const bytes = cryptoApi.getRandomValues(new Uint8Array(16));
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    const hex = Array.from(bytes, byte => byte.toString(16).padStart(2, '0'));
    return `${hex.slice(0, 4).join('')}-${hex.slice(4, 6).join('')}-${hex.slice(6, 8).join('')}-${hex.slice(8, 10).join('')}-${hex.slice(10).join('')}`;
  }

  fallbackIdSequence += 1;
  return `local-${Date.now().toString(36)}-${fallbackIdSequence.toString(36)}-${Math.random().toString(36).slice(2)}`;
}

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}
