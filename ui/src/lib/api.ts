// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : ui/src/lib/api.ts
//  Author : Praneeth
//  License: MIT
//  Purpose: Typed bridge to the Rust backend (Tauri invoke/listen). When the UI
//           runs in a plain browser (`npm run dev` without Tauri) it falls back
//           to a mock backend so the interface can still be developed.
// =============================================================================
import type { SendSpec, SerialAccess, SerialPortInfo, SessionConfig, SessionInfo } from "./forge.svelte";
import type {
  ConnectionSettings,
  DiscoveredServer,
  DnsResult,
  Hop,
  NetConnection,
  NetDevice,
  PerfProgress,
  PerfResult,
  PerfServerEvent,
  PerfServerInfo,
  PingSummary,
  PortResult,
  TestSpec,
  WifiNetwork,
  Alert,
  AlertConfig,
  SearchOptions,
  SearchProgress,
  SearchSummary,
  Service,
  ServiceAction,
  ServiceDetails,
  Socket,
  StartupEntry,
  StartupInput,
  CleanReport,
  CleanTarget,
  FileActionReport,
  Package,
  PackageSource,
  ProcessResources,
  RemovalOutcome,
  RemovalPlan,
  ScanOptions,
  ScanProgress,
  ScanResult,
  HwSection,
  KillOutcome,
  ProcessDetails,
  ProcessInfo,
  Signal,
  SystemSummary,
  Telemetry,
} from "./types";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(cmd, args);
  }
  const mock = await import("./mock");
  return mock.invoke<T>(cmd, args);
}

export const api = {
  systemSummary: () => call<SystemSummary>("system_summary"),
  hardwareInventory: (refresh = false) => call<HwSection[]>("hardware_inventory", { refresh }),
  listProcesses: () => call<ProcessInfo[]>("list_processes"),
  processDetails: (pid: number) => call<ProcessDetails>("process_details", { pid }),
  signalProcess: (pid: number, signal: Signal) => call<KillOutcome>("signal_process", { pid, signal }),
  setRefreshInterval: (ms: number) => call<void>("set_refresh_interval", { ms }),
  processResources: (pid: number) => call<ProcessResources>("process_resources", { pid }),
  setProcessPriority: (pid: number, nice: number) => call<boolean>("set_process_priority", { pid, nice }),

  listPackages: () => call<Package[]>("list_packages"),
  packageIcon: (icon: string) => call<string | null>("package_icon", { icon }),
  planPackageRemoval: (source: PackageSource, names: string[]) => call<RemovalPlan>("plan_package_removal", { source, names }),
  removePackages: (source: PackageSource, names: string[], purge: boolean) =>
    call<RemovalOutcome>("remove_packages", { source, names, purge }),

  cleanerScan: () => call<CleanTarget[]>("cleaner_scan"),
  cleanerClean: (ids: string[]) => call<CleanReport>("cleaner_clean", { ids }),

  largeScan: (options: ScanOptions) => call<ScanResult>("large_scan", { options }),
  largeCancel: () => call<void>("large_cancel"),
  filesTrash: (paths: string[]) => call<FileActionReport>("files_trash", { paths }),
  filesDelete: (paths: string[], asAdmin: boolean) => call<FileActionReport>("files_delete", { paths, asAdmin }),
  revealPath: (path: string) => call<void>("reveal_path", { path }),

  searchFiles: (options: SearchOptions) => call<SearchSummary>("search_files", { options }),
  searchCancel: () => call<void>("search_cancel"),
  openPath: (path: string) => call<void>("open_path", { path }),

  listServices: (user: boolean) => call<Service[]>("list_services", { user }),
  serviceDetails: (unit: string, user: boolean) => call<ServiceDetails>("service_details", { unit, user }),
  serviceAction: (unit: string, action: ServiceAction, user: boolean) => call<string>("service_action", { unit, action, user }),

  listStartup: () => call<StartupEntry[]>("list_startup"),
  setStartupEnabled: (id: string, enabled: boolean) => call<void>("set_startup_enabled", { id, enabled }),
  saveStartup: (entry: StartupInput) => call<string>("save_startup", { entry }),
  removeStartup: (id: string) => call<void>("remove_startup", { id }),

  listConnections: () => call<Socket[]>("list_connections"),

  getAlertConfig: () => call<AlertConfig>("get_alert_config"),
  setAlertConfig: (config: AlertConfig) => call<void>("set_alert_config", { config }),
  exportText: (path: string, contents: string) => call<void>("export_text", { path, contents }),

  netDevices: () => call<NetDevice[]>("net_devices"),
  netConnections: () => call<NetConnection[]>("net_connections"),
  netConnectionSettings: (uuid: string) => call<ConnectionSettings>("net_connection_settings", { uuid }),
  netUpdateConnection: (settings: ConnectionSettings, applyNow: boolean) => call<string>("net_update_connection", { settings, applyNow }),
  netActivate: (uuid: string, up: boolean) => call<void>("net_activate", { uuid, up }),
  wifiScan: (rescan: boolean) => call<WifiNetwork[]>("wifi_scan", { rescan }),
  wifiRadio: (on: boolean | null) => call<boolean>("wifi_radio", { on }),

  netPing: (host: string, count: number, intervalMs: number) => call<PingSummary>("net_ping", { host, count, intervalMs }),
  netTrace: (host: string) => call<Hop[]>("net_trace", { host }),
  netToolCancel: () => call<void>("net_tool_cancel"),
  netLookup: (host: string) => call<DnsResult>("net_lookup", { host }),
  netPorts: (host: string, ports: number[], timeoutMs: number) => call<PortResult[]>("net_ports", { host, ports, timeoutMs }),

  perfServerStart: (port: number, code: string | null) => call<PerfServerInfo>("perf_server_start", { port, code }),
  perfServerStop: () => call<void>("perf_server_stop"),
  perfServerStatus: () => call<PerfServerInfo | null>("perf_server_status"),
  perfDiscover: (port: number) => call<DiscoveredServer[]>("perf_discover", { port }),
  perfRun: (target: string, port: number, code: string | null, spec: TestSpec) => call<PerfResult>("perf_run", { target, port, code, spec }),
  perfCancel: () => call<void>("perf_cancel"),

  forgePorts: () => call<SerialPortInfo[]>("forge_ports"),
  forgeSerialAccess: () => call<SerialAccess>("forge_serial_access"),
  forgeGrantSerial: () => call<string>("forge_grant_serial"),
  forgeOpen: (config: SessionConfig) => call<SessionInfo>("forge_open", { config }),
  forgeClose: (id: number) => call<void>("forge_close", { id }),
  forgeEncode: (spec: SendSpec) => call<{ hex: string; length: number; error: string | null }>("forge_encode", { spec }),
  forgeSend: (id: number, spec: SendSpec, target: string | null) => call<number>("forge_send", { id, spec, target }),
  forgeRepeat: (id: number, spec: SendSpec, target: string | null, intervalMs: number, count: number | null) =>
    call<void>("forge_repeat", { id, spec, target, intervalMs, count }),
  forgeStopJobs: (id: number) => call<void>("forge_stop_jobs", { id }),
  forgeSendFile: (id: number, path: string, target: string | null, chunk: number, delayMs: number) =>
    call<number>("forge_send_file", { id, path, target, chunk, delayMs }),
  forgeLines: (id: number, dtr: boolean | null, rts: boolean | null) => call<void>("forge_lines", { id, dtr, rts }),
  forgeBreak: (id: number, ms: number) => call<void>("forge_break", { id, ms }),
  forgeLog: (id: number, path: string | null) => call<void>("forge_log", { id, path }),
};

/** Open-file dialog; returns null when cancelled or unavailable. */
export async function pickFile(): Promise<string | null> {
  if (!isTauri) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ multiple: false, directory: false });
  return typeof r === "string" ? r : null;
}

/** Generic Tauri event subscription (no-op in browser mode). */
export async function onEvent<T>(name: string, handler: (payload: T) => void): Promise<() => void> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<T>(name, (e) => handler(e.payload));
}

export type { PerfProgress, PerfServerEvent };

/** Save-file dialog; returns null when cancelled or unavailable. */
export async function pickSaveFile(defaultPath: string, ext: string): Promise<string | null> {
  if (!isTauri) return null;
  const { save } = await import("@tauri-apps/plugin-dialog");
  return (await save({ defaultPath, filters: [{ name: ext.toUpperCase(), extensions: [ext] }] })) ?? null;
}

/** Subscribe to file-search progress (incremental hit batches). */
export async function onSearchProgress(handler: (p: SearchProgress) => void): Promise<() => void> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<SearchProgress>("search-progress", (e) => handler(e.payload));
}

/** Subscribe to threshold alerts raised by the backend. */
export async function onAlert(handler: (a: Alert) => void): Promise<() => void> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<Alert>("alert", (e) => handler(e.payload));
}

/** Folder picker (Tauri dialog); returns null when cancelled or unavailable. */
export async function pickFolder(defaultPath?: string): Promise<string | null> {
  if (!isTauri) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ directory: true, multiple: false, defaultPath });
  return typeof r === "string" ? r : null;
}

/** Subscribe to large-file scan progress events. */
export async function onScanProgress(handler: (p: ScanProgress) => void): Promise<() => void> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<ScanProgress>("largefiles-progress", (e) => handler(e.payload));
}

/** Subscribe to the 1 Hz telemetry stream. Returns an unsubscribe function. */
export async function onTelemetry(handler: (t: Telemetry) => void): Promise<() => void> {
  if (isTauri) {
    const { listen } = await import("@tauri-apps/api/event");
    return listen<Telemetry>("telemetry", (e) => handler(e.payload));
  }
  const mock = await import("./mock");
  return mock.subscribe(handler);
}
