// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : ui/src/lib/launchers.ts
//  Author : Praneeth
//  License: MIT
//  Purpose: App Launchers - types and backend calls.
// =============================================================================
import { call } from "./api";

export type AppKind = "native" | "appImage" | "script" | "python" | "jar";

export interface Candidate {
  path: string;
  rel: string;
  kind: AppKind;
  score: number;
  reason: string;
  executable: boolean;
  size: number;
}

export interface IconFile {
  path: string;
  rel: string;
  preview: string | null;
  width: number;
  height: number;
  score: number;
}

export interface ScanResult {
  folder: string;
  name: string;
  candidates: Candidate[];
  icons: IconFile[];
  libraryDirs: string[];
  qtPluginDir: string | null;
  venvPython: string | null;
  pyinstaller: boolean;
  files: number;
  bytes: number;
  truncated: boolean;
}

export interface EnvVar {
  name: string;
  value: string;
}

export interface LauncherSpec {
  id: string;
  name: string;
  comment: string;
  folder: string;
  program: string;
  kind: AppKind;
  interpreter: string | null;
  args: string;
  workdir: string;
  env: EnvVar[];
  terminal: boolean;
  category: string;
  wmClass: string;
  icon: string;
  menu: boolean;
  desktop: boolean;
  dock: boolean;
  grantAccess: boolean;
  grantWrite: boolean;
}

export interface UserAccount {
  name: string;
  uid: number;
  gid: number;
  home: string;
  fullName: string;
  loggedIn: boolean;
  isAdmin: boolean;
}

export interface Issue {
  level: "error" | "warning" | "fix";
  user: string | null;
  text: string;
}

export interface CheckReport {
  issues: Issue[];
  preview: string;
  commandLine: string;
  needsAccess: string[];
  missingLibraries: string[];
}

export interface UserResult {
  user: string;
  ok: boolean;
  done: string[];
  warnings: string[];
  error: string | null;
  aclParents: string[];
  aclFolder: boolean;
}

export interface LauncherRecord {
  spec: LauncherSpec;
  users: string[];
  results: UserResult[];
  created: number;
  updated: number;
  iconPreview: string | null;
}

export const CATEGORIES: [string, string][] = [
  ["Utility", "Accessories"],
  ["Development", "Programming"],
  ["Science", "Science & Engineering"],
  ["Network", "Internet"],
  ["Graphics", "Graphics"],
  ["AudioVideo", "Sound & Video"],
  ["Office", "Office"],
  ["Education", "Education"],
  ["Game", "Games"],
  ["System", "System Tools"],
  ["Settings", "Settings"],
];

export const KIND_LABEL: Record<AppKind, string> = {
  native: "Program",
  appImage: "AppImage",
  script: "Script",
  python: "Python",
  jar: "Java",
};

export const ln = {
  users: () => call<{ users: UserAccount[]; me: string; aclAvailable: boolean }>("launcher_users"),
  scan: (folder: string) =>
    call<{ scan: ScanResult; suggestedId: string; suggestedEnv: EnvVar[]; takenIds: string[] }>("launcher_scan", { folder }),
  check: (spec: LauncherSpec, users: string[]) => call<CheckReport>("launcher_check", { spec, users }),
  iconPreview: (path: string) => call<string | null>("launcher_icon_preview", { path }),
  create: (spec: LauncherSpec, users: string[]) => call<UserResult[]>("launcher_create", { spec, users }),
  list: () => call<LauncherRecord[]>("launcher_list"),
  remove: (id: string, users: string[], revokeAccess: boolean) => call<UserResult[]>("launcher_remove", { id, users, revokeAccess }),
  testRun: (spec: LauncherSpec) => call<{ running: boolean; exitCode: number | null; output: string }>("launcher_test_run", { spec }),
};
