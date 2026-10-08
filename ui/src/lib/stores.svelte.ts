// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : ui/src/lib/stores.svelte.ts
//  Author : Praneeth
//  License: MIT
//  Purpose: Global reactive state: latest telemetry, rolling history buffers
//           for charts, static system summary and user settings.
// =============================================================================
import { api, onAlert, onTelemetry } from "./api";
import type { SystemSummary, Telemetry } from "./types";

export type Theme = "dark" | "light" | "system";

export interface Settings {
  refreshMs: number;
  historyLength: number;
  theme: Theme;
  /** Show process CPU as share of the whole machine instead of one core. */
  normalizeProcessCpu: boolean;
}

const SETTINGS_KEY = "syspulse.settings";
const DEFAULTS: Settings = { refreshMs: 1000, historyLength: 120, theme: "dark", normalizeProcessCpu: false };

function loadSettings(): Settings {
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    if (raw) return { ...DEFAULTS, ...JSON.parse(raw) };
  } catch {
    /* storage unavailable: use defaults */
  }
  return { ...DEFAULTS };
}

export interface GpuHistory {
  util: (number | null)[];
  vram: (number | null)[];
  temp: (number | null)[];
  power: (number | null)[];
}

export interface History {
  /** Unix seconds for each sample. */
  time: number[];
  cpu: number[];
  perCore: number[][];
  mem: number[];
  swap: number[];
  netRx: number[];
  netTx: number[];
  diskRead: number[];
  diskWrite: number[];
  cpuTemp: (number | null)[];
  gpus: Record<string, GpuHistory>;
}

const emptyHistory = (): History => ({
  time: [], cpu: [], perCore: [], mem: [], swap: [], netRx: [], netTx: [],
  diskRead: [], diskWrite: [], cpuTemp: [], gpus: {},
});

function push<T>(arr: T[], v: T, cap: number): T[] {
  const next = arr.length >= cap ? arr.slice(arr.length - cap + 1) : arr.slice();
  next.push(v);
  return next;
}

export interface Toast {
  id: number;
  text: string;
  kind: "ok" | "error" | "info";
}

class AppState {
  latest = $state.raw<Telemetry | null>(null);
  history = $state.raw<History>(emptyHistory());
  summary = $state.raw<SystemSummary | null>(null);
  settings = $state<Settings>(loadSettings());
  page = $state<string>("dashboard");
  toasts = $state<Toast[]>([]);
  #toastId = 0;

  /** Show a transient notification (bottom-right). */
  notify(text: string, kind: Toast["kind"] = "ok", ms = 4500) {
    const id = ++this.#toastId;
    this.toasts = [...this.toasts, { id, text, kind }];
    setTimeout(() => (this.toasts = this.toasts.filter((t) => t.id !== id)), ms);
  }

  #unlisten: (() => void) | null = null;

  async start() {
    this.applyTheme();
    try {
      this.summary = await api.systemSummary();
    } catch (e) {
      console.error("system_summary failed", e);
    }
    await api.setRefreshInterval(this.settings.refreshMs).catch(() => {});
    this.#unlisten = await onTelemetry((t) => this.ingest(t));
    await onAlert((a) => this.notify(`${a.title}: ${a.body}`, "error", 10000));
  }

  stop() {
    this.#unlisten?.();
  }

  ingest(t: Telemetry) {
    const cap = this.settings.historyLength;
    const h = this.history;
    const net = t.network.reduce((a, n) => ({ rx: a.rx + n.rxBps, tx: a.tx + n.txBps }), { rx: 0, tx: 0 });
    const disk = t.diskIo.reduce((a, d) => ({ r: a.r + d.readBps, w: a.w + d.writeBps }), { r: 0, w: 0 });
    const m = t.memory;

    const perCore = t.cpu.perCore.map((v, i) => push(h.perCore[i] ?? [], v, cap));
    const gpus: Record<string, GpuHistory> = {};
    for (const g of t.gpus) {
      const prev = h.gpus[g.id] ?? { util: [], vram: [], temp: [], power: [] };
      const vramPct = g.vramUsed != null && g.vramTotal ? (g.vramUsed / g.vramTotal) * 100 : null;
      gpus[g.id] = {
        util: push(prev.util, g.utilization, cap),
        vram: push(prev.vram, vramPct, cap),
        temp: push(prev.temp, g.temperature, cap),
        power: push(prev.power, g.powerWatts, cap),
      };
    }

    this.history = {
      time: push(h.time, t.timestamp / 1000, cap),
      cpu: push(h.cpu, t.cpu.total, cap),
      perCore,
      mem: push(h.mem, m.total ? (m.used / m.total) * 100 : 0, cap),
      swap: push(h.swap, m.swapTotal ? (m.swapUsed / m.swapTotal) * 100 : 0, cap),
      netRx: push(h.netRx, net.rx, cap),
      netTx: push(h.netTx, net.tx, cap),
      diskRead: push(h.diskRead, disk.r, cap),
      diskWrite: push(h.diskWrite, disk.w, cap),
      cpuTemp: push(h.cpuTemp, t.cpu.temperature, cap),
      gpus,
    };
    this.latest = t;
  }

  saveSettings() {
    try {
      localStorage.setItem(SETTINGS_KEY, JSON.stringify(this.settings));
    } catch {
      /* ignore */
    }
    this.applyTheme();
    api.setRefreshInterval(this.settings.refreshMs).catch(() => {});
  }

  applyTheme() {
    let theme = this.settings.theme;
    if (theme === "system") {
      theme = window.matchMedia?.("(prefers-color-scheme: light)").matches ? "light" : "dark";
    }
    document.documentElement.dataset.theme = theme;
  }
}

export const app = new AppState();
