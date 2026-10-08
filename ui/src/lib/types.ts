// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : ui/src/lib/types.ts
//  Author : Praneeth
//  License: MIT
//  Purpose: TypeScript mirrors of the Rust models in crates/core/src/models.rs.
//           Keep both files in sync (serde uses camelCase).
// =============================================================================

export interface SystemSummary {
  hostname: string;
  user: string;
  osName: string;
  osVersion: string;
  kernel: string;
  arch: string;
  desktop: string | null;
  sessionType: string | null;
  cpuModel: string;
  physicalCores: number | null;
  logicalCores: number;
  totalMemory: number;
  bootTime: number;
}

export interface CpuSample {
  total: number;
  perCore: number[];
  frequencyMhz: number[];
  loadAverage: [number, number, number];
  temperature: number | null;
  processCount: number;
  uptimeSecs: number;
}

export interface MemorySample {
  total: number;
  used: number;
  available: number;
  cached: number;
  swapTotal: number;
  swapUsed: number;
}

export interface FilesystemSample {
  device: string;
  mountPoint: string;
  fsType: string;
  total: number;
  available: number;
  removable: boolean;
}

export interface DiskIoSample {
  device: string;
  readBps: number;
  writeBps: number;
  busyPercent: number;
}

export interface NetworkSample {
  interface: string;
  rxBps: number;
  txBps: number;
  rxTotal: number;
  txTotal: number;
}

export interface SensorSample {
  chip: string;
  label: string;
  temperature: number;
  critical: number | null;
}

export interface BatterySample {
  percent: number;
  status: string;
  powerWatts: number | null;
}

export type GpuVendor = "nvidia" | "amd" | "intel" | "other";

export interface GpuProcess {
  pid: number;
  name: string;
  usedMemory: number | null;
  kind: "graphics" | "compute";
}

export interface GpuSample {
  id: string;
  vendor: GpuVendor;
  name: string;
  driver: string | null;
  driverVersion: string | null;
  utilization: number | null;
  memoryUtilization: number | null;
  vramUsed: number | null;
  vramTotal: number | null;
  temperature: number | null;
  powerWatts: number | null;
  powerLimitWatts: number | null;
  coreClockMhz: number | null;
  maxCoreClockMhz: number | null;
  memoryClockMhz: number | null;
  fanPercent: number | null;
  encoderUtilization: number | null;
  decoderUtilization: number | null;
  pcieGeneration: number | null;
  pcieWidth: number | null;
  /** Runtime power state, e.g. "active" or "suspended" (sleeping dGPU). */
  powerState: string | null;
  processes: GpuProcess[];
}

export interface Telemetry {
  timestamp: number;
  cpu: CpuSample;
  memory: MemorySample;
  filesystems: FilesystemSample[];
  diskIo: DiskIoSample[];
  network: NetworkSample[];
  sensors: SensorSample[];
  battery: BatterySample | null;
  gpus: GpuSample[];
}

export interface ProcessInfo {
  pid: number;
  ppid: number | null;
  name: string;
  user: string;
  state: string;
  cpu: number;
  memory: number;
  virtualMemory: number;
  startTime: number;
  diskReadBps: number;
  diskWriteBps: number;
  threads: number | null;
  nice: number | null;
  command: string;
  exe: string | null;
}

export interface ProcessDetails {
  info: ProcessInfo;
  cmdline: string[];
  cwd: string | null;
  root: string | null;
  uid: number | null;
  gid: number | null;
  openFiles: number | null;
  cgroup: string | null;
  children: number[];
  environmentCount: number | null;
  status: [string, string][];
}

export type Signal = "term" | "kill" | "stop" | "cont" | "hup" | "int";

export interface KillOutcome {
  pid: number;
  signal: Signal;
  elevated: boolean;
}

export interface HwProperty {
  key: string;
  value: string;
}

export interface HwDevice {
  title: string;
  subtitle: string | null;
  properties: HwProperty[];
}

export interface HwSection {
  id: string;
  title: string;
  devices: HwDevice[];
}

// ---------------------------------------------------------------- Phase 2 ---

export interface OpenFile {
  fd: number;
  target: string;
  kind: "file" | "socket" | "pipe" | "device" | "anon" | "other";
}

export interface Connection {
  protocol: string;
  local: string;
  remote: string;
  state: string;
}

export interface ProcessResources {
  files: OpenFile[];
  connections: Connection[];
  totalFds: number;
  permissionDenied: boolean;
}

export type PackageSource = "apt" | "snap" | "flatpak";

export interface Package {
  id: string;
  source: PackageSource;
  name: string;
  displayName: string;
  version: string;
  summary: string;
  installedSize: number | null;
  installDate: number | null;
  section: string | null;
  arch: string | null;
  publisher: string | null;
  homepage: string | null;
  isApp: boolean;
  icon: string | null;
  protected: boolean;
  autoInstalled: boolean;
  installation: string | null;
}

export interface RemovalPlan {
  source: PackageSource;
  requested: string[];
  removes: string[];
  freedBytes: number;
  needsAdmin: boolean;
  blockers: string[];
  warnings: string[];
}

export interface RemovalOutcome {
  message: string;
  output: string;
}

export type CleanCategory = "system" | "user" | "browser" | "developer";

export interface CleanTarget {
  id: string;
  title: string;
  description: string;
  category: CleanCategory;
  paths: string[];
  size: number;
  files: number;
  needsAdmin: boolean;
  caution: string | null;
  selectedByDefault: boolean;
  details: string[];
}

export interface CleanReport {
  freedBytes: number;
  cleaned: string[];
  errors: string[];
}

export type FileCategory = "video" | "audio" | "image" | "archive" | "diskimage" | "document" | "package" | "code" | "other";

export interface ScanOptions {
  root: string;
  minSize: number;
  includeHidden: boolean;
  sameFilesystem: boolean;
  categories: FileCategory[];
  maxResults: number;
}

export interface LargeFile {
  path: string;
  name: string;
  size: number;
  modified: number | null;
  category: FileCategory;
  owned: boolean;
}

export interface FolderSize {
  path: string;
  size: number;
  files: number;
}

export interface ScanResult {
  root: string;
  files: LargeFile[];
  folders: FolderSize[];
  subfolders: FolderSize[];
  categories: { category: FileCategory; bytes: number; files: number }[];
  totalFiles: number;
  totalBytes: number;
  skippedDirs: number;
  elapsedMs: number;
  cancelled: boolean;
  tree: DirNode;
}

export interface ScanProgress {
  filesScanned: number;
  bytesScanned: number;
  current: string;
  elapsedMs: number;
}

export interface FileActionReport {
  done: string[];
  freedBytes: number;
  denied: string[];
  errors: string[];
}

// ---------------------------------------------------------------- Phase 3 ---

export interface DirNode {
  name: string;
  path: string;
  size: number;
  kind: "dir" | "files" | "other";
  children: DirNode[];
}

export type MatchMode = "contains" | "glob" | "regex" | "exact";

export interface SearchOptions {
  root: string;
  query: string;
  mode: MatchMode;
  caseSensitive: boolean;
  matchPath: boolean;
  kind: "any" | "files" | "folders";
  categories: FileCategory[];
  extensions: string[];
  minSize: number | null;
  maxSize: number | null;
  modifiedAfter: number | null;
  modifiedBefore: number | null;
  owner: "any" | "me" | "others";
  includeHidden: boolean;
  skipNoise: boolean;
  sameFilesystem: boolean;
  content: string | null;
  maxResults: number;
}

export interface SearchHit {
  path: string;
  name: string;
  isDir: boolean;
  size: number | null;
  modified: number | null;
  category: FileCategory | null;
  owner: string;
  content: { line: number; text: string; matches: number } | null;
}

export interface SearchProgress {
  scanned: number;
  matched: number;
  current: string;
  elapsedMs: number;
  batch: SearchHit[];
}

export interface SearchSummary {
  root: string;
  hits: SearchHit[];
  scanned: number;
  truncated: boolean;
  cancelled: boolean;
  elapsedMs: number;
}

export interface Service {
  unit: string;
  description: string;
  load: string;
  active: string;
  sub: string;
  fileState: string | null;
  preset: string | null;
  user: boolean;
  protected: boolean;
}

export type ServiceAction = "start" | "stop" | "restart" | "enable" | "disable";

export interface ServiceDetails {
  unit: string;
  mainPid: number | null;
  memory: number | null;
  cpuSeconds: number | null;
  tasks: number | null;
  restarts: number | null;
  since: string | null;
  fragmentPath: string | null;
  properties: [string, string][];
  logs: string[];
}

export interface StartupEntry {
  id: string;
  name: string;
  comment: string;
  exec: string;
  icon: string | null;
  enabled: boolean;
  system: boolean;
  userFile: boolean;
  noDisplay: boolean;
  appliesHere: boolean;
  delay: number | null;
}

export interface StartupInput {
  id: string | null;
  name: string;
  exec: string;
  comment: string;
  delay: number | null;
}

export interface Socket {
  protocol: string;
  localAddress: string;
  localPort: number;
  remoteAddress: string;
  remotePort: number;
  state: string;
  pid: number | null;
  process: string | null;
  user: string;
  scope: "local" | "listening" | "remote";
}

export interface AlertConfig {
  enabled: boolean;
  cpuTemp: number;
  gpuTemp: number;
  memoryPercent: number;
  diskPercent: number;
  batteryPercent: number;
  sustainSecs: number;
  cooldownSecs: number;
}

export interface Alert {
  key: string;
  title: string;
  body: string;
  level: "warning" | "critical";
}

// -------------------------------------------------------- Network (Phase 3.5) ---

export interface WifiLink { ssid: string; signal: number | null; bitrate: string | null; frequencyMhz: number | null; security: string | null }

export interface NetDevice {
  device: string;
  kind: string;
  state: string;
  connection: string | null;
  connectionUuid: string | null;
  mac: string | null;
  mtu: number | null;
  ipv4: string[];
  ipv4Gateway: string | null;
  ipv4Dns: string[];
  ipv6: string[];
  ipv6Gateway: string | null;
  ipv6Dns: string[];
  routes: string[];
  carrier: boolean | null;
  speedMbps: number | null;
  driver: string | null;
  wifi: WifiLink | null;
}

export interface NetConnection { name: string; uuid: string; kind: string; device: string | null; active: boolean; autoconnect: boolean }

export interface IpSettings { method: string; addresses: string[]; gateway: string | null; dns: string[]; ignoreAutoDns: boolean }

export interface ConnectionSettings {
  uuid: string;
  name: string;
  kind: string;
  interfaceName: string | null;
  autoconnect: boolean;
  mtu: number | null;
  ssid: string | null;
  ipv4: IpSettings;
  ipv6: IpSettings;
}

export interface WifiNetwork {
  inUse: boolean;
  ssid: string;
  bssid: string;
  signal: number;
  channel: number;
  frequencyMhz: number;
  band: string;
  rate: string;
  security: string;
  knownUuid: string | null;
}

export interface PingSummary {
  host: string;
  address: string | null;
  sent: number;
  received: number;
  lossPercent: number;
  minMs: number | null;
  avgMs: number | null;
  maxMs: number | null;
  mdevMs: number | null;
  rtts: (number | null)[];
}
export interface Hop { ttl: number; host: string | null; rttMs: number | null; note: string }
export interface DnsResult { host: string; addresses: string[]; elapsedMs: number; servers: string[] }
export interface PortResult { port: number; state: "open" | "closed" | "filtered"; elapsedMs: number }

export type PerfKind = "tcp" | "udp" | "latency";
export type PerfDirection = "upload" | "download" | "bidir";

export interface TestSpec {
  kind: PerfKind;
  direction: PerfDirection;
  streams: number;
  durationSecs: number;
  bitrateMbps: number;
  packetSize: number;
  intervalMs: number;
  latencyPps: number;
}

export interface PerfInterval {
  index: number;
  t: number;
  bytes: number;
  mbps: number;
  packets: number | null;
  lost: number | null;
  jitterMs: number | null;
  retransmits: number | null;
  rttMs: number | null;
}

export interface PerfProgress {
  dir: "upload" | "download" | "latency";
  index: number;
  t: number;
  mbps: number;
  bytes: number;
  retransmits: number | null;
  rttMs: number | null;
  lost: number | null;
  jitterMs: number | null;
}

export interface DirectionResult {
  direction: "upload" | "download";
  bytes: number;
  seconds: number;
  mbpsAvg: number;
  mbpsMin: number;
  mbpsMax: number;
  mbpsStddev: number;
  intervals: PerfInterval[];
  retransmits: number | null;
  rttMs: number | null;
  packetsSent: number | null;
  packetsReceived: number | null;
  lost: number | null;
  lossPercent: number | null;
  outOfOrder: number | null;
  jitterMs: number | null;
}

export interface LatencyResult {
  sent: number;
  received: number;
  lossPercent: number;
  minMs: number;
  avgMs: number;
  maxMs: number;
  p50Ms: number;
  p90Ms: number;
  p99Ms: number;
  stddevMs: number;
  jitterMs: number;
  samples: (number | null)[];
}

export interface PerfResult {
  server: string;
  serverHost: string;
  spec: TestSpec;
  startedAt: number;
  upload: DirectionResult | null;
  download: DirectionResult | null;
  latency: LatencyResult | null;
  cancelled: boolean;
}

export interface PerfServerInfo { port: number; host: string; addresses: string[]; code: string | null }
export interface DiscoveredServer { address: string; host: string; port: number; codeRequired: boolean }

export type PerfServerEvent =
  | { kind: "testStarted"; peer: string; host: string; spec: TestSpec }
  | { kind: "interval"; dir: PerfDirection; t: number; mbps: number }
  | { kind: "testFinished"; peer: string; summary: string }
  | { kind: "rejected"; peer: string; reason: string }
  | { kind: "error"; message: string };
