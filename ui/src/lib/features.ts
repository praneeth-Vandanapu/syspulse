// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : ui/src/lib/features.ts
//  Author : Praneeth
//  License: MIT
//  Purpose: Feature switches for release builds. Keep in sync with the
//           backend switches (e.g. recorder::ENABLED in src-tauri/src/recorder.rs).
// =============================================================================

export const FEATURES = {
  /** Screen recorder: hidden in public releases until it is finished. */
  recorder: false,
};
