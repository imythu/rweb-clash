import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"

export const SUB_DELIMITER = ' ^_^ ';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}
