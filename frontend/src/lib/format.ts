/** Display-only formatting helpers for the launcher UI. */

import { m } from '@/paraglide/messages.js';

export function agoLabel(unix: number): string {
  const secs = Math.max(0, Date.now() / 1000 - unix);
  const mins = Math.round(secs / 60);
  if (mins < 1) return m['app.time.just_now']();
  if (mins < 60) return m['app.time.minutes']({ count: mins });
  const hours = Math.round(mins / 60);
  if (hours < 24) return m['app.time.hours']({ count: hours });
  const days = Math.floor(hours / 24);
  if (days === 1) return m['app.time.yesterday']();
  return m['app.time.days']({ count: days });
}

export function bytes(n: number): string {
  const units = ['B', 'KB', 'MB', 'GB'];
  let value = n;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${value.toFixed(value < 10 && unit > 0 ? 1 : 0)} ${units[unit]}`;
}

const BYTE_UNITS = ['B', 'KB', 'MB', 'GB'];

/** `current / total` sharing the total's unit: `120 / 340 MB`. */
export function bytesOf(current: number, total: number): string {
  let unit = 0;
  while (total / 1024 ** (unit + 1) >= 1 && unit < BYTE_UNITS.length - 1) {
    unit++;
  }
  const scale = 1024 ** unit;
  const digits = total / scale < 10 && unit > 0 ? 1 : 0;
  return `${(current / scale).toFixed(digits)} / ${(total / scale).toFixed(digits)} ${BYTE_UNITS[unit]}`;
}

const MEMORY_UNIT_GB: Record<string, number> = { k: 1024 ** 2, m: 1024, g: 1 };

/**
 * Gigabytes parsed from a memory setting like `4G` or `8192M` (defaults to 4).
 * The daemon's vocabulary is a number plus a unit, so the unit has to be read —
 * dropping it reads a megabyte value as that many gigabytes. Rounded to a whole
 * gigabyte because that is the granularity every surface here offers, and a
 * fractional value would be written back as one the daemon rejects.
 */
export function memGb(memory: string): number {
  const match = /^\s*(\d+)\s*([kmg])\s*$/i.exec(memory);
  if (!match) return 4;
  const gb = Number(match[1]) / MEMORY_UNIT_GB[match[2].toLowerCase()];
  return Math.max(1, Math.round(gb));
}

/** A running duration in seconds as a compact `1d 3h` / `12m 5s` label. */
export function uptime(totalSeconds: number): string {
  const s = Math.max(0, Math.floor(totalSeconds));
  const days = Math.floor(s / 86400);
  const hours = Math.floor((s % 86400) / 3600);
  const minutes = Math.floor((s % 3600) / 60);
  const seconds = s % 60;
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  if (minutes > 0) return `${minutes}m ${seconds}s`;
  return `${seconds}s`;
}

export function compact(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return `${n}`;
}

/** A byte throughput as a compact `1.2 MB/s`. */
export function bytesPerSecond(rate: number): string {
  return `${bytes(rate)}/s`;
}

const RATE_WINDOW_MS = 500;

/**
 * Byte rate measured over fixed minimum windows, mirroring the CLI's
 * `RateMeter` (`crates/cli/src/ui/progress.rs`). Progress events arrive in
 * bursts (several within a frame), so a per-event instantaneous rate is
 * dominated by intra-burst spikes and wildly overstates throughput; averaging
 * each ≥`RATE_WINDOW_MS` span weights fast and stalled periods by their real
 * duration. A counter that goes backwards is a new stream: the rate resets and
 * nothing shows until the new one has sustained a full window — a file served
 * instantly from the local cache never displays a speed at all.
 */
export class RateMeter {
  private window: { at: number; count: number } | null = null;
  private perSecond = 0;

  observe(current: number, now: number = performance.now()): number {
    if (this.window && current >= this.window.count) {
      const elapsed = now - this.window.at;
      if (elapsed >= RATE_WINDOW_MS) {
        const rate = ((current - this.window.count) * 1000) / elapsed;
        this.perSecond =
          this.perSecond === 0 ? rate : 0.5 * this.perSecond + 0.5 * rate;
        this.window = { at: now, count: current };
      }
    } else {
      this.perSecond = 0;
      this.window = { at: now, count: current };
    }
    return this.perSecond;
  }

  reset(): void {
    this.window = null;
    this.perSecond = 0;
  }
}
