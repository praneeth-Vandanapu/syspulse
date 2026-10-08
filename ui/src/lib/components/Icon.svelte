<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/lib/components/Icon.svelte
  Author : Praneeth
  License: MIT
  Purpose: Minimal stroke icon set (24x24 grid) used across the app.
-->
<script lang="ts">
  const PATHS: Record<string, string> = {
    dashboard: "M3 13h8V3H3zM13 21h8V11h-8zM3 21h8v-6H3zM13 3v6h8V3z",
    cpu: "M9 3v2M15 3v2M9 19v2M15 19v2M3 9h2M3 15h2M19 9h2M19 15h2M7 5h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2zM9 9h6v6H9z",
    gpu: "M2 7h18a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H2zM2 4v16M6 18v3M10 18v3M14 18v3M15.5 12.5m-2.5 0a2.5 2.5 0 1 0 5 0a2.5 2.5 0 1 0-5 0M7 11h3v3H7z",
    memory: "M4 7h16a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V8a1 1 0 0 1 1-1zM7 17v3M12 17v3M17 17v3M7 10v4M11 10v4M15 10v4",
    process: "M4 6h16M4 12h16M4 18h10",
    chip: "M6 4h12v16H6zM9 8h6M9 12h6M9 16h3",
    package: "M12 2l9 5v10l-9 5-9-5V7zM12 22V12M21 7l-9 5-9-5M7.5 4.5l9 5",
    broom: "M19 3l-8 8M9.5 9.5l5 5M4 20c1-4 3-7 5.5-9.5l4 4C11 17 8 19 4 20zM7 17l2-2",
    files: "M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9zM14 3v6h6M8 13h8M8 17h5",
    search: "M11 4a7 7 0 1 0 0 14a7 7 0 1 0 0-14zM21 21l-5-5",
    lens: "M10.5 3.5a7 7 0 1 0 0 14a7 7 0 1 0 0-14zM21 21l-5.5-5.5M6 11h2l1.2-2.5 2 5 1.3-2.5H15",
    record: "M12 12m-4 0a4 4 0 1 0 8 0a4 4 0 1 0-8 0M12 12m-9 0a9 9 0 1 0 18 0a9 9 0 1 0-18 0",
    settings: "M12 15a3 3 0 1 0 0-6a3 3 0 0 0 0 6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-2.9 1.2V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-2.9-1.2l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1A1.7 1.7 0 0 0 3 15H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.2-2.9l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1A1.7 1.7 0 0 0 10 3.1V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 2.9 1.2l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0 1.2 2.9H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z",
    disk: "M12 5m-8 0a8 3 0 1 0 16 0a8 3 0 1 0-16 0M4 5v14c0 1.7 3.6 3 8 3s8-1.3 8-3V5M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3",
    network: "M2 9a15 15 0 0 1 20 0M5 12.5a10 10 0 0 1 14 0M8.5 16a5 5 0 0 1 7 0M12 19.5h.01",
    thermo: "M14 14.8V4a2 2 0 1 0-4 0v10.8a4 4 0 1 0 4 0z",
    battery: "M3 7h15a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V8a1 1 0 0 1 1-1zM22 11v2",
    bolt: "M13 2L4 14h7l-1 8 9-12h-7z",
    refresh: "M21 12a9 9 0 1 1-3-6.7L21 8M21 3v5h-5",
    close: "M6 6l12 12M18 6L6 18",
    stop: "M6 6h12v12H6z",
    play: "M7 4l13 8-13 8z",
    skull: "M12 3a8 8 0 0 0-8 8c0 2.6 1.3 4.6 3 5.8V20h10v-3.2c1.7-1.2 3-3.2 3-5.8a8 8 0 0 0-8-8zM9 11.5h.01M15 11.5h.01M10 20v-2M14 20v-2",
    info: "M12 12m-9 0a9 9 0 1 0 18 0a9 9 0 1 0-18 0M12 16v-5M12 8h.01",
    check: "M5 12l5 5L20 7",
    alert: "M12 3l10 18H2zM12 10v4M12 17h.01",
    usb: "M12 2v14M9 5l3-3 3 3M7 10v2l5 3M17 8v3l-5 3M12 19m-2 0a2 2 0 1 0 4 0a2 2 0 1 0-4 0",
    audio: "M4 9v6h4l5 4V5L8 9zM16 9a4 4 0 0 1 0 6M19 6a8 8 0 0 1 0 12",
    board: "M3 3h18v18H3zM7 7h4v4H7zM15 7h2M15 11h2M7 15h10",
    sun: "M12 12m-4 0a4 4 0 1 0 8 0a4 4 0 1 0-8 0M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4",
    moon: "M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z",
    sort: "M8 4v16M4 8l4-4 4 4M16 20V4M12 16l4 4 4-4",
  };

  let { name, size = 18, stroke = 1.8 }: { name: string; size?: number; stroke?: number } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={stroke}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  <path d={PATHS[name] ?? PATHS.info} />
</svg>
