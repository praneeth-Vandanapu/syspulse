// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : ui/src/lib/format.ts
//  Author : Praneeth
//  License: MIT
//  Purpose: Human-readable formatting helpers (bytes, rates, durations).
// =============================================================================

const UNITS = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];

export function bytes(n: number | null | undefined, digits = 1): string {
  if (n == null || !isFinite(n)) return "—";
  let i = 0;
  let v = Math.abs(n);
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(i === 0 ? 0 : digits)} ${UNITS[i]}`;
}

export function rate(bps: number | null | undefined): string {
  if (bps == null) return "—";
  return `${bytes(bps)}/s`;
}

export function pct(n: number | null | undefined, digits = 0): string {
  return n == null ? "—" : `${n.toFixed(digits)}%`;
}

export function temp(c: number | null | undefined): string {
  return c == null ? "—" : `${c.toFixed(0)} °C`;
}

export function mhz(n: number | null | undefined): string {
  if (n == null) return "—";
  return n >= 1000 ? `${(n / 1000).toFixed(2)} GHz` : `${n.toFixed(0)} MHz`;
}

export function watts(n: number | null | undefined): string {
  return n == null ? "—" : `${n.toFixed(1)} W`;
}

export function duration(secs: number): string {
  secs = Math.max(0, Math.floor(secs));
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  if (d > 0) return `${d}d ${h}h ${m}m`;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

export function dateTime(unixSecs: number): string {
  return new Date(unixSecs * 1000).toLocaleString();
}

/** Status level for a 0-100 load value; used with an icon + label, never colour alone. */
export function level(v: number | null | undefined, warn = 70, crit = 90): "good" | "warning" | "critical" {
  if (v == null) return "good";
  if (v >= crit) return "critical";
  if (v >= warn) return "warning";
  return "good";
}
