import { type ClassValue, clsx } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/**
 * Positions `0..count` for a fixed set of interchangeable elements (placeholder
 * bones, slider thumbs): the position is the element's identity, so it is the
 * React key.
 */
export function slots(count: number): number[] {
  return Array.from({ length: count }, (_, slot) => slot);
}
