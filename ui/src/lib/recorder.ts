// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : ui/src/lib/recorder.ts
//  Author : Praneeth
//  License: MIT
//  Purpose: Screen-recorder types (mirrors src-tauri/src/recorder.rs and
//           crates/core/src/recorder) and API wrappers.
// =============================================================================
import { isTauri } from "./api";

export interface MonitorInfo { name: string; x: number; y: number; width: number; height: number; primary: boolean }
export interface WindowInfo { xid: string; title: string; app: string; x: number; y: number; width: number; height: number }
export interface AudioDevice { name: string; description: string; isMonitor: boolean; isDefault: boolean; available: boolean; isBluetooth: boolean }
export interface AudioState { devices: AudioDevice[]; currentOutput: AudioDevice | null; currentInput: AudioDevice | null }
/** Device values that follow the system's current default output / input. */
export const FOLLOW_OUTPUT = "@DEFAULT_MONITOR@";
export const FOLLOW_INPUT = "@DEFAULT_SOURCE@";
export interface EncoderInfo { element: string; codec: string; hardware: boolean; label: string }
export interface Sources {
  sessionType: string;
  supported: boolean;
  monitors: MonitorInfo[];
  windows: WindowInfo[];
  audio: AudioDevice[];
  encoders: EncoderInfo[];
  missing: string[];
}

export type CaptureArea =
  | { kind: "monitor"; name: string; x: number; y: number; width: number; height: number }
  | { kind: "window"; xid: string; title: string; width: number; height: number }
  | { kind: "region"; x: number; y: number; width: number; height: number };

export interface RecordConfig {
  area: CaptureArea;
  audio: { system: string | null; mic: string | null; systemVolume: number; micVolume: number };
  video: { container: "mp4" | "mkv" | "webm"; codec: "h264" | "h265" | "vp8"; fps: number; quality: "standard" | "high" | "max"; encoder: string; showPointer: boolean };
  outputDir: string;
}

export interface StartOptions { config: RecordConfig; countdown: number; hideWindow: boolean }
export interface RecordingInfo { path: string; encoder: string; width: number; height: number; fps: number; bitrateKbps: number; audio: string[] }
export interface Status { state: "idle" | "countdown" | "recording" | "paused"; elapsed: number; size: number; info: RecordingInfo | null }
export interface RecordingResult { path: string; durationSecs: number; size: number; encoder: string }
export interface RecordingFile { path: string; name: string; size: number; modified: number }
export interface RegionSnapshot { image: string; x: number; y: number; width: number; height: number }
export interface Rect { x: number; y: number; width: number; height: number }
export type RecorderEvent = { type: "level"; system: number | null; mic: number | null } | { type: "error"; message: string };

/** Browser-preview data (never used in the desktop app). */
function demo(cmd: string): unknown {
  const config: RecordConfig = {
    area: { kind: "monitor", name: "eDP-1", x: 0, y: 0, width: 1920, height: 1080 },
    audio: { system: "alsa_output.speaker.monitor", mic: "alsa_input.jack", systemVolume: 1, micVolume: 1 },
    video: { container: "mp4", codec: "h264", fps: 30, quality: "high", encoder: "auto", showPointer: true },
    outputDir: "~/Videos/SysPulse",
  };
  switch (cmd) {
    case "rec_sources":
      return {
        sessionType: "x11", supported: true, missing: [],
        monitors: [{ name: "eDP-1", x: 0, y: 0, width: 1920, height: 1080, primary: true }, { name: "HDMI-1", x: 1920, y: 0, width: 2560, height: 1440, primary: false }],
        windows: [{ xid: "0x3a00007", title: "Visual Studio Code", app: "Code", x: 0, y: 0, width: 1850, height: 1053 }],
        audio: [
          { name: "alsa_input.mic", description: "Digital Microphone", isMonitor: false, isDefault: true, available: true, isBluetooth: false },
          { name: "alsa_input.jack", description: "Headphones Stereo Microphone", isMonitor: false, isDefault: false, available: false, isBluetooth: false },
          { name: "bluez_sink.buds.monitor", description: "Monitor of OnePlus Nord Buds 4 Pro", isMonitor: true, isDefault: true, available: true, isBluetooth: true },
          { name: "alsa_output.speaker.monitor", description: "Monitor of Speaker + Headphones", isMonitor: true, isDefault: false, available: true, isBluetooth: false },
        ],
        encoders: [{ element: "x264enc", codec: "h264", hardware: false, label: "x264 H.264 (CPU)" }, { element: "x265enc", codec: "h265", hardware: false, label: "x265 H.265 (CPU)" }],
      } satisfies Sources;
    case "rec_defaults":
      return { config, countdown: 3, hideWindow: true } satisfies StartOptions;
    case "rec_audio": {
      const d = (demo("rec_sources") as Sources).audio;
      return { devices: d, currentOutput: d[2], currentInput: d[0] } satisfies AudioState;
    }
    case "rec_status":
      return { state: "idle", elapsed: 0, size: 0, info: null } satisfies Status;
    case "rec_list":
      return [{ path: "/home/praneeth/Videos/SysPulse/SysPulse_2026-10-08_12-00-10.mp4", name: "SysPulse_2026-10-08_12-00-10.mp4", size: 4_027_515, modified: 1.7599e9 }];
    default:
      return undefined;
  }
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri) return demo(cmd) as T;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

export const rec = {
  sources: () => call<Sources>("rec_sources"),
  audio: () => call<AudioState>("rec_audio"),
  defaults: () => call<StartOptions>("rec_defaults"),
  saveDefaults: (options: StartOptions) => call<void>("rec_save_defaults", { options }),
  start: (options: StartOptions) => call<void>("rec_start", { options }),
  togglePause: () => call<void>("rec_toggle_pause"),
  stop: (save: boolean) => call<void>("rec_stop", { save }),
  status: () => call<Status>("rec_status"),
  list: (dir: string) => call<RecordingFile[]>("rec_list", { dir }),
  meter: (system: string | null, mic: string | null) => call<void>("rec_meter", { system, mic }),
  pickRegion: (m: { x: number; y: number; width: number; height: number }) => call<void>("rec_pick_region", m),
  regionSnapshot: () => call<RegionSnapshot>("rec_region_snapshot"),
  regionDone: (rect: Rect | null) => call<void>("rec_region_done", { rect }),
};
