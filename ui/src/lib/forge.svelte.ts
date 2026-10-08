// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : ui/src/lib/forge.svelte.ts
//  Author : Praneeth
//  License: MIT
//  Purpose: PacketForge client state: open sessions, received/sent records,
//           framing (chunk / newline / delimiter / idle gap / fixed length),
//           counters, macros and saved profiles. Events arrive in batches
//           from the backend (`forge-events`) and keep being collected while
//           the page is not visible.
// =============================================================================
import { isTauri, onEvent } from "./api";
import { fromBase64 } from "./forgefmt";

// ----------------------------------------------------------------- types ---

export type SessionConfig =
  | { kind: "tcpClient"; host: string; port: number; autoReconnect: boolean }
  | { kind: "tcpServer"; bind: string; port: number; maxClients: number }
  | { kind: "udp"; bind: string; localPort: number; remoteHost: string | null; remotePort: number | null; broadcast: boolean; multicastGroup: string | null }
  | { kind: "serial"; path: string; baud: number; dataBits: number; parity: "none" | "odd" | "even"; stopBits: number; flow: "none" | "hardware" | "software"; dtr: boolean; rts: boolean };

export interface SessionInfo {
  id: number;
  title: string;
  config: SessionConfig;
  rxBytes: number;
  txBytes: number;
}

export type InputMode = "text" | "hex";
export type LineEnding = "none" | "cr" | "lf" | "crlf";
export type Checksum = "none" | "crc16-modbus" | "crc16-ccitt" | "crc32" | "xor8" | "sum8" | "lrc";

export interface SendSpec {
  mode: InputMode;
  data: string;
  lineEnding: LineEnding;
  checksum: Checksum;
  hercules: boolean;
}

export interface SerialPortInfo {
  path: string;
  kind: "usb" | "pci" | "bluetooth" | "builtin";
  description: string;
  vid: number | null;
  pid: number | null;
  serialNumber: string | null;
  manufacturer: string | null;
  product: string | null;
  accessible: boolean;
}

export interface SerialAccess {
  user: string;
  group: string;
  inGroup: boolean;
  needsRelogin: boolean;
}

export type ForgeEvent =
  | { type: "data"; session: number; dir: "rx" | "tx"; peer: string | null; t: number; data: string }
  | { type: "state"; session: number; state: "connecting" | "open" | "listening" | "closed" | "error"; message: string }
  | { type: "clients"; session: number; clients: string[] }
  | { type: "lines"; session: number; cts: boolean; dsr: boolean; dcd: boolean; ri: boolean; dtr: boolean; rts: boolean }
  | { type: "job"; session: number; kind: "repeat" | "file"; active: boolean; sent: number; total: number | null };

export interface Rec {
  seq: number;
  dir: "rx" | "tx" | "info";
  peer: string | null;
  /** First byte time (ms). */
  t: number;
  bytes: Uint8Array;
  /** Still receiving (framing not complete yet). */
  open: boolean;
  /** Info records (connect/disconnect) carry text instead of bytes. */
  note?: string;
  level?: string;
}

export type FramingMode = "chunk" | "newline" | "delimiter" | "gap" | "length";
export interface Framing {
  mode: FramingMode;
  delimiter: number;
  gapMs: number;
  length: number;
}

export interface Macro {
  id: string;
  name: string;
  spec: SendSpec;
  key: string | null; // "F1".."F12"
}

export interface Profile {
  id: string;
  name: string;
  config: SessionConfig;
}

// ------------------------------------------------------------- session ---

const MAX_RECORDS = 50_000;
const MAX_OPEN_RECORD = 4096;
/** Newline / delimiter framing: close a line anyway after this much silence,
 *  so devices that never send the delimiter still get one line per message. */
const LINE_IDLE_MS = 500;

interface Stream {
  rec: Rec | null;
  lastT: number;
}

export class SessionView {
  id: number;
  title: string;
  config: SessionConfig;
  state = $state<string>("connecting");
  message = $state("");
  clients = $state<string[]>([]);
  lines = $state<{ cts: boolean; dsr: boolean; dcd: boolean; ri: boolean; dtr: boolean; rts: boolean } | null>(null);
  repeat = $state<{ active: boolean; sent: number } | null>(null);
  file = $state<{ active: boolean; sent: number; total: number } | null>(null);
  logPath = $state<string | null>(null);
  /** Bumped when records change; components re-render from it. */
  version = $state(0);
  rx = $state(0);
  tx = $state(0);
  rxRecs = $state(0);
  txRecs = $state(0);
  // Default: one record per message, where a message ends after a short pause.
  // Works for devices with and without line endings, and keeps packets that
  // arrive in several reads (typical on serial) together.
  framing = $state<Framing>({ mode: "gap", delimiter: 0x0a, gapMs: 20, length: 16 });

  records: Rec[] = [];
  startT = Date.now();
  #seq = 0;
  #streams = new Map<string, Stream>();
  #mode: FramingMode = "gap";

  constructor(info: SessionInfo) {
    this.id = info.id;
    this.title = info.title;
    this.config = info.config;
    // UDP datagrams are natural frames.
    if (info.config.kind === "udp") this.framing.mode = "chunk";
  }

  #push(r: Rec) {
    this.records.push(r);
    if (this.records.length > MAX_RECORDS) this.records.splice(0, this.records.length - MAX_RECORDS);
  }

  info(note: string, level = "info") {
    this.#push({ seq: ++this.#seq, dir: "info", peer: null, t: Date.now(), bytes: new Uint8Array(), open: false, note, level });
  }

  /** Append bytes to the open record of a stream (creating it if needed). */
  #append(s: Stream, dir: "rx" | "tx", peer: string | null, t: number, bytes: Uint8Array) {
    if (!s.rec) {
      s.rec = { seq: ++this.#seq, dir, peer, t, bytes, open: true };
      this.#push(s.rec);
    } else {
      const merged = new Uint8Array(s.rec.bytes.length + bytes.length);
      merged.set(s.rec.bytes);
      merged.set(bytes, s.rec.bytes.length);
      s.rec.bytes = merged;
    }
    if (s.rec.bytes.length >= MAX_OPEN_RECORD) this.#close(s);
  }

  #close(s: Stream) {
    if (s.rec) s.rec.open = false;
    s.rec = null;
  }

  data(dir: "rx" | "tx", peer: string | null, t: number, bytes: Uint8Array) {
    if (dir === "rx") this.rx += bytes.length;
    else this.tx += bytes.length;
    // Sent data: one record per send.
    if (dir === "tx") {
      this.txRecs++;
      this.#push({ seq: ++this.#seq, dir, peer, t, bytes, open: false });
      return;
    }
    const key = peer ?? "";
    let s = this.#streams.get(key);
    if (!s) {
      s = { rec: null, lastT: t };
      this.#streams.set(key, s);
    }
    this.#syncMode();
    const f = this.framing;
    const before = this.records.length;
    switch (f.mode) {
      case "chunk":
        this.#close(s);
        this.#push({ seq: ++this.#seq, dir, peer, t, bytes, open: false });
        break;
      case "gap":
        if (s.rec && t - s.lastT > f.gapMs) this.#close(s);
        this.#append(s, dir, peer, t, bytes);
        break;
      case "length": {
        let i = 0;
        while (i < bytes.length) {
          const room = f.length - (s.rec?.bytes.length ?? 0);
          const part = bytes.subarray(i, i + room);
          this.#append(s, dir, peer, t, part);
          i += part.length;
          if ((s.rec?.bytes.length ?? 0) >= f.length) this.#close(s);
        }
        break;
      }
      default: {
        const delim = f.mode === "newline" ? 0x0a : f.delimiter;
        let from = 0;
        for (let i = 0; i < bytes.length; i++) {
          if (bytes[i] === delim) {
            this.#append(s, dir, peer, t, bytes.subarray(from, i + 1));
            this.#close(s);
            from = i + 1;
          }
        }
        if (from < bytes.length) this.#append(s, dir, peer, t, bytes.subarray(from));
      }
    }
    s.lastT = t;
    this.rxRecs += Math.max(0, this.records.length - before);
  }

  /** A framing change applies immediately: finish any line still open. */
  #syncMode() {
    if (this.framing.mode === this.#mode) return false;
    this.#mode = this.framing.mode;
    let changed = false;
    for (const s of this.#streams.values()) {
      changed ||= !!s.rec;
      this.#close(s);
    }
    return changed;
  }

  /**
   * Close open records that have gone quiet (gap mode: after the pause;
   * newline/delimiter: after LINE_IDLE_MS) so the next data starts a new,
   * separately timestamped record. Returns true when something changed.
   */
  tick(now: number): boolean {
    let changed = this.#syncMode();
    const idle = this.framing.mode === "gap" ? this.framing.gapMs : this.framing.mode === "newline" || this.framing.mode === "delimiter" ? LINE_IDLE_MS : null;
    if (idle == null) return changed;
    for (const s of this.#streams.values()) {
      if (s.rec && now - s.lastT > idle) {
        this.#close(s);
        changed = true;
      }
    }
    return changed;
  }

  clear() {
    this.records = [];
    this.#streams.clear();
    this.rxRecs = this.txRecs = 0;
    this.startT = Date.now();
    this.version++;
  }
}

// --------------------------------------------------------------- store ---

function uid() {
  return Math.random().toString(36).slice(2, 10);
}

class ForgeStore {
  sessions = $state<SessionView[]>([]);
  active = $state<number | null>(null);
  macros = $state<Macro[]>([]);
  profiles = $state<Profile[]>([]);
  history = $state<SendSpec[]>([]);
  #started = false;
  #dirty = new Set<SessionView>();

  async start() {
    if (this.#started) return;
    this.#started = true;
    if (!isTauri) return this.#demo();
    const { invoke } = await import("@tauri-apps/api/core");
    const stored = (await invoke<Record<string, unknown>>("forge_store_load").catch(() => ({}))) as { macros?: Macro[]; profiles?: Profile[]; history?: SendSpec[] };
    this.macros = stored.macros ?? [];
    this.profiles = stored.profiles ?? [];
    this.history = stored.history ?? [];
    // Sessions survive page switches; pick up any that are open.
    const open = await invoke<SessionInfo[]>("forge_sessions").catch(() => []);
    for (const i of open) if (!this.byId(i.id)) this.sessions = [...this.sessions, new SessionView(i)];
    if (this.sessions.length && this.active == null) this.active = this.sessions[0].id;

    await onEvent<ForgeEvent[]>("forge-events", (batch) => this.#ingest(batch));
    // Gap framing timeout + throttled re-render (at most ~10 fps).
    setInterval(() => {
      const now = Date.now();
      for (const s of this.sessions) if (s.tick(now)) this.#dirty.add(s);
      for (const s of this.#dirty) s.version++;
      this.#dirty.clear();
    }, 100);
  }

  /** Browser-preview only: a fake session with sample traffic. */
  #demo() {
    const v = this.add({ id: 1, title: "TCP → 192.168.1.50:502", config: { kind: "tcpClient", host: "192.168.1.50", port: 502, autoReconnect: false }, rxBytes: 0, txBytes: 0 });
    const enc = (s: string) => new TextEncoder().encode(s);
    const t0 = Date.now() - 5000;
    v.info("connected to 192.168.1.50:502", "open");
    v.data("tx", null, t0, enc("AT\r\n"));
    v.data("rx", null, t0 + 40, enc("OK\r\n"));
    v.data("tx", null, t0 + 900, new Uint8Array([1, 3, 0, 0, 0, 10, 0xc5, 0xcd]));
    v.data("rx", null, t0 + 950, new Uint8Array([1, 3, 20, 0, 0x2a, 0, 0x10, 0xff, 0xfe, 0x1b, 0x5b, 0x30, 0x6d, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x8a, 0x3c]));
    // Packets without any line ending, 300 ms apart: each must get its own line.
    v.data("rx", null, t0 + 2000, enc("PKT-1 temp=24.5"));
    v.data("rx", null, t0 + 2300, enc("PKT-2 temp=24.6"));
    v.data("rx", null, t0 + 2600, enc("PKT-3 temp=24.7"));
    // One packet split across two reads 5 ms apart stays together.
    v.data("rx", null, t0 + 3000, enc("PKT-4 split "));
    v.data("rx", null, t0 + 3005, enc("across reads"));
    v.version++;
  }

  byId(id: number) {
    return this.sessions.find((s) => s.id === id);
  }

  get current(): SessionView | null {
    return this.sessions.find((s) => s.id === this.active) ?? null;
  }

  #ingest(batch: ForgeEvent[]) {
    for (const e of batch) {
      const s = this.byId(e.session);
      if (!s) continue;
      switch (e.type) {
        case "data":
          s.data(e.dir, e.peer, e.t, fromBase64(e.data));
          this.#dirty.add(s);
          break;
        case "state":
          // Errors (e.g. a failed send) are logged but do not change the link state.
          if (e.state !== "error") s.state = e.state;
          s.message = e.message;
          s.info(e.message, e.state);
          this.#dirty.add(s);
          break;
        case "clients":
          s.clients = e.clients;
          break;
        case "lines":
          s.lines = { cts: e.cts, dsr: e.dsr, dcd: e.dcd, ri: e.ri, dtr: e.dtr, rts: e.rts };
          break;
        case "job":
          if (e.kind === "repeat") s.repeat = e.active ? { active: true, sent: e.sent } : null;
          else s.file = e.active ? { active: true, sent: e.sent, total: e.total ?? 0 } : null;
          break;
      }
    }
  }

  add(info: SessionInfo) {
    const v = new SessionView(info);
    v.state = info.config.kind === "tcpServer" || info.config.kind === "udp" ? "listening" : "open";
    this.sessions = [...this.sessions, v];
    this.active = v.id;
    return v;
  }

  remove(id: number) {
    this.sessions = this.sessions.filter((s) => s.id !== id);
    if (this.active === id) this.active = this.sessions[0]?.id ?? null;
  }

  async persist() {
    if (!isTauri) return;
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("forge_store_save", {
      data: $state.snapshot({ macros: this.macros, profiles: this.profiles, history: this.history.slice(0, 30) }),
    }).catch(() => {});
  }

  remember(spec: SendSpec) {
    const key = `${spec.mode}|${spec.data}|${spec.lineEnding}|${spec.checksum}`;
    this.history = [spec, ...this.history.filter((h) => `${h.mode}|${h.data}|${h.lineEnding}|${h.checksum}` !== key)].slice(0, 30);
    this.persist();
  }

  saveMacro(name: string, spec: SendSpec, key: string | null, id?: string) {
    const m: Macro = { id: id ?? uid(), name, spec, key };
    this.macros = id ? this.macros.map((x) => (x.id === id ? m : x)) : [...this.macros, m];
    if (key) this.macros = this.macros.map((x) => (x.id !== m.id && x.key === key ? { ...x, key: null } : x));
    this.persist();
  }

  deleteMacro(id: string) {
    this.macros = this.macros.filter((m) => m.id !== id);
    this.persist();
  }

  saveProfile(name: string, config: SessionConfig) {
    this.profiles = [...this.profiles.filter((p) => p.name !== name), { id: uid(), name, config }];
    this.persist();
  }

  deleteProfile(id: string) {
    this.profiles = this.profiles.filter((p) => p.id !== id);
    this.persist();
  }
}

export const forge = new ForgeStore();
