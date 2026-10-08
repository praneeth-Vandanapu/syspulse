// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : ui/src/lib/netlens.ts
//  Author : Praneeth
//  License: MIT
//  Purpose: NetLens (packet capture & analysis) types, backend calls and
//           ready-made filter presets.
// =============================================================================
import { call, isTauri } from "./api";

export interface InterfaceInfo {
  name: string;
  kind: "ethernet" | "wifi" | "loopback" | "bridge" | "virtual" | "tunnel" | "other";
  up: boolean;
  mac: string;
  addresses: string[];
  rxBytes: number;
  txBytes: number;
  linktype: number | null;
}

export interface CaptureAccess {
  helperFound: boolean;
  helperPath: string;
  canCapture: boolean;
  filtersAvailable: boolean;
  message: string;
}

export interface Setup {
  access: CaptureAccess;
  tools: { tshark: string | null; wireshark: string | null };
  interfaces: InterfaceInfo[];
}

export interface CaptureSpec {
  interfaces: string[];
  filter: string;
  promiscuous: boolean;
  snaplen: number;
}

export interface Limits {
  maxPackets: number;
  maxMb: number;
  maxSeconds: number;
}

export interface Status {
  state: "idle" | "capturing" | "loading" | "stopped";
  source: string;
  interfaces: string[];
  captureFilter: string;
  packets: number;
  shown: number;
  bytes: number;
  memory: number;
  dropped: number;
  duration: number;
  packetsPerSec: number;
  bitsPerSec: number;
  messages: string[];
  displayFilter: string;
  unsaved: boolean;
  progress: number;
  stopReason: string;
}

export interface Row {
  no: number;
  time: number;
  delta: number;
  clock: string;
  src: string;
  dst: string;
  proto: string;
  len: number;
  info: string;
  color: string;
  problem: 0 | 1 | 2;
  app: string;
  dir: 0 | 1 | 2;
}

export interface Node {
  label: string;
  field: string;
  filter?: string;
  start: number;
  len: number;
  level: number;
  children?: Node[];
}

export interface PacketDetail {
  no: number;
  summary: string;
  proto: string;
  tree: Node[];
  bytes: string; // base64
  stream: number | null;
  tcp: boolean;
  time: string;
}

export interface Conversation {
  id: number;
  transport: "TCP" | "UDP";
  protocol: string;
  a: string;
  aPort: number;
  b: string;
  bPort: number;
  host: string;
  app: string;
  packetsAb: number;
  packetsBa: number;
  bytesAb: number;
  bytesBa: number;
  start: number;
  duration: number;
  state: string;
  problems: number;
  service: string;
}

export interface AppUsage {
  name: string;
  pids: number[];
  connections: number;
  packets: number;
  bytesOut: number;
  bytesIn: number;
  hosts: string[];
  problems: number;
}

export interface Stats {
  hierarchy: { path: string; name: string; depth: number; packets: number; bytes: number }[];
  ioPackets: number[];
  ioBits: number[];
  totalPackets: number;
  totalBytes: number;
  duration: number;
  protocols: [string, number, number][];
}

export interface Insight {
  id: string;
  severity: "error" | "warning" | "info";
  title: string;
  detail: string;
  count: number;
  filter: string;
}

export interface RtpStat {
  ssrc: number;
  src: string;
  dst: string;
  payloadType: number;
  codec: string;
  clock: number;
  packets: number;
  bytes: number;
  lost: number;
  sequenceErrors: number;
  maxJitterMs: number;
  meanJitterMs: number;
  maxDeltaMs: number;
  frames: number;
  keyFrames: number;
  firstNs: number;
  lastNs: number;
  firstFrame: number;
}

export interface Chunk {
  fromA: boolean;
  frame: number;
  data: string; // base64
  gap: number;
}

export interface FollowResult {
  stream: number;
  tcp: boolean;
  a: string;
  b: string;
  bytesA: number;
  bytesB: number;
  chunks: Chunk[];
  truncated: boolean;
  filter: string;
}

export interface FieldInfo {
  name: string;
  kind: "proto" | "uint" | "float" | "bool" | "str" | "ip" | "mac" | "bytes";
  description: string;
}

export const nl = {
  setup: () => call<Setup>("netlens_setup"),
  interfaces: () => call<InterfaceInfo[]>("netlens_interfaces"),
  grant: (grant: boolean) => call<string>("netlens_grant", { grant }),
  start: (spec: CaptureSpec, limits: Limits) => call<void>("netlens_start", { spec, limits }),
  stop: () => call<void>("netlens_stop"),
  clear: () => call<void>("netlens_clear"),
  status: () => call<Status>("netlens_status"),
  rows: (start: number, count: number, resolve: boolean) => call<Row[]>("netlens_rows", { start, count, resolve }),
  detail: (no: number) => call<PacketDetail | null>("netlens_detail", { no }),
  setFilter: (text: string) => call<number>("netlens_set_filter", { text }),
  checkFilter: (text: string) => call<string | null>("netlens_check_filter", { text }),
  checkCaptureFilter: (text: string) => call<string | null>("netlens_check_capture_filter", { text }),
  fields: () => call<FieldInfo[]>("netlens_fields"),
  conversations: () => call<Conversation[]>("netlens_conversations"),
  apps: () => call<AppUsage[]>("netlens_apps"),
  stats: () => call<Stats>("netlens_stats"),
  insights: () => call<Insight[]>("netlens_insights"),
  rtp: () => call<RtpStat[]>("netlens_rtp"),
  follow: (stream: number) => call<FollowResult | null>("netlens_follow", { stream }),
  locate: (no: number) => call<number | null>("netlens_locate", { no }),
  open: (path: string) => call<void>("netlens_open", { path }),
  save: (path: string, displayedOnly: boolean) => call<number>("netlens_save", { path, displayedOnly }),
  wiresharkDecode: (no: number) => call<Node[]>("netlens_wireshark_decode", { no }),
  openWireshark: (displayedOnly: boolean) => call<string>("netlens_open_wireshark", { displayedOnly }),
};

/** Capture-file picker (pcap / pcapng). */
export async function pickCapture(): Promise<string | null> {
  if (!isTauri) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({
    multiple: false,
    directory: false,
    filters: [
      { name: "Capture files", extensions: ["pcapng", "pcap", "cap"] },
      { name: "All files", extensions: ["*"] },
    ],
  });
  return typeof r === "string" ? r : null;
}

export async function pickCaptureSave(defaultName: string): Promise<string | null> {
  if (!isTauri) return null;
  const { save } = await import("@tauri-apps/plugin-dialog");
  return (await save({ defaultPath: defaultName, filters: [{ name: "pcapng", extensions: ["pcapng"] }] })) ?? null;
}

export const DISPLAY_PRESETS: { label: string; filter: string; hint: string }[] = [
  { label: "HTTP", filter: "http", hint: "Plain web traffic (requests and responses)" },
  { label: "HTTPS", filter: "tls || quic", hint: "Encrypted web traffic: TLS over TCP and QUIC (HTTP/3)" },
  { label: "TLS handshakes", filter: "tls.handshake.type in {1 2}", hint: "Client/Server Hello: which sites are opened" },
  { label: "RTSP", filter: "rtsp", hint: "Camera / stream control (DESCRIBE, SETUP, PLAY...)" },
  { label: "Media (RTP)", filter: "rtp || rtcp", hint: "Audio/video packets and their quality reports" },
  { label: "DNS", filter: "dns", hint: "Name lookups" },
  { label: "TCP", filter: "tcp", hint: "All TCP" },
  { label: "UDP", filter: "udp", hint: "All UDP" },
  { label: "Connections opened", filter: "tcp.flags.syn == 1 && tcp.flags.ack == 0", hint: "TCP handshakes started" },
  {
    label: "Problems",
    filter:
      "tcp.analysis.flags || tcp.flags.reset == 1 || dns.flags.rcode != 0 || http.response.code >= 400 || rtsp.status >= 400 || tls.alert_message.level == 2 || icmp.type == 3",
    hint: "Everything NetLens flagged as an error or warning",
  },
];

export const CAPTURE_PRESETS: { label: string; filter: string }[] = [
  { label: "Everything", filter: "" },
  { label: "Web (HTTP + HTTPS)", filter: "tcp port 80 or tcp port 443 or udp port 443" },
  { label: "RTSP + RTP (cameras)", filter: "tcp port 554 or tcp port 8554 or (udp and not port 53 and not port 443)" },
  { label: "DNS only", filter: "port 53 or port 5353" },
  { label: "TCP only", filter: "tcp" },
  { label: "UDP only", filter: "udp" },
  { label: "Exclude SSH", filter: "not port 22" },
  { label: "One host…", filter: "host 192.168.1.1" },
];

export function b64bytes(s: string): Uint8Array {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export function bits(bps: number): string {
  if (!bps) return "0 bit/s";
  const u = ["bit/s", "kbit/s", "Mbit/s", "Gbit/s"];
  let i = 0;
  let v = bps;
  while (v >= 1000 && i < u.length - 1) {
    v /= 1000;
    i++;
  }
  return `${v.toFixed(i ? 1 : 0)} ${u[i]}`;
}

export function secs(t: number): string {
  if (t < 60) return `${t.toFixed(1)} s`;
  const m = Math.floor(t / 60);
  const s = Math.floor(t % 60);
  if (m < 60) return `${m} min ${s} s`;
  return `${Math.floor(m / 60)} h ${m % 60} min`;
}
