<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/App.svelte
  Author : Praneeth
  License: MIT
  Purpose: Application shell: sidebar navigation, header and page router.
-->
<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import Icon from "./lib/components/Icon.svelte";
  import ComingSoon from "./lib/components/ComingSoon.svelte";
  import { app } from "./lib/stores.svelte";
  import { duration } from "./lib/format";
  import { isTauri } from "./lib/api";
  import Dashboard from "./pages/Dashboard.svelte";
  import Performance from "./pages/Performance.svelte";
  import Gpu from "./pages/Gpu.svelte";
  import Processes from "./pages/Processes.svelte";
  import Hardware from "./pages/Hardware.svelte";
  import Settings from "./pages/Settings.svelte";
  import Software from "./pages/Software.svelte";
  import Cleaner from "./pages/Cleaner.svelte";
  import LargeFiles from "./pages/LargeFiles.svelte";
  import Toasts from "./lib/components/Toasts.svelte";
  import Search from "./pages/Search.svelte";
  import Services from "./pages/Services.svelte";
  import Startup from "./pages/Startup.svelte";
  import Connections from "./pages/Connections.svelte";
  import NetworkSettings from "./pages/NetworkSettings.svelte";
  import SpeedTest from "./pages/SpeedTest.svelte";
  import NetTools from "./pages/NetTools.svelte";
  import PacketForge from "./pages/PacketForge.svelte";
  import NetLens from "./pages/NetLens.svelte";
  import Launchers from "./pages/Launchers.svelte";
  import Recorder from "./pages/Recorder.svelte";
  import { FEATURES } from "./lib/features";
  import { onEvent } from "./lib/api";

  interface NavItem { id: string; label: string; icon: string; soon?: boolean }
  const NAV: { group: string; items: NavItem[] }[] = [
    {
      group: "Monitor",
      items: [
        { id: "dashboard", label: "Dashboard", icon: "dashboard" },
        { id: "performance", label: "Performance", icon: "cpu" },
        { id: "gpu", label: "GPU", icon: "gpu" },
        { id: "processes", label: "Processes", icon: "process" },
      ],
    },
    {
      group: "Network",
      items: [
        { id: "netsettings", label: "Network Settings", icon: "network" },
        { id: "speedtest", label: "Speed Test", icon: "gpu" },
        { id: "nettools", label: "Net Tools", icon: "search" },
        { id: "packetforge", label: "PacketForge", icon: "bolt" },
        { id: "netlens", label: "NetLens", icon: "lens" },
        { id: "connections", label: "Connections", icon: "usb" },
      ],
    },
    {
      group: "System",
      items: [
        { id: "hardware", label: "Hardware", icon: "chip" },
        { id: "services", label: "Services", icon: "settings" },
        { id: "startup", label: "Startup Apps", icon: "play" },
        { id: "launchers", label: "App Launchers", icon: "apps" },
      ],
    },
    {
      group: "Maintenance",
      items: [
        { id: "software", label: "Software", icon: "package" },
        { id: "cleaner", label: "Cache Cleaner", icon: "broom" },
        { id: "largefiles", label: "Large Files", icon: "files" },
        { id: "search", label: "File Search", icon: "search" },
      ],
    },
    ...(FEATURES.recorder ? [{ group: "Media", items: [{ id: "recorder", label: "Screen Recorder", icon: "record" }] }] : []),
  ];

  const SOON: Record<string, { icon: string; title: string; phase: string; features: string[] }> = {
  };

  // Browser demo mode only (never in the real app): "?page=<id>" opens a page
  // directly and "&shot" hides the demo badge - used for README screenshots.
  const demoParams = !isTauri ? new URLSearchParams(location.search) : null;
  const screenshotMode = !!demoParams?.has("shot");
  if (demoParams?.get("page")) app.page = demoParams.get("page")!;

  onMount(() => {
    app.start();
    // Backend can ask for a page (e.g. back to the recorder after a recording).
    onEvent<string>("navigate", (page) => (app.page = page));
  });
  onDestroy(() => app.stop());

  let uptime = $derived(app.latest ? duration(app.latest.cpu.uptimeSecs) : "");
</script>

<div class="shell">
  <aside>
    <div class="brand">
      <div class="logo">
        <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="white" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M3 13h4l2-6 3 11 2.5-8 1.5 3h5" /></svg>
      </div>
      <div class="name">SysPulse</div>
    </div>

    <nav>
      {#each NAV as g (g.group)}
        <div class="group">{g.group}</div>
        {#each g.items as item (item.id)}
          <button class="nav" class:active={app.page === item.id} onclick={() => (app.page = item.id)} aria-current={app.page === item.id ? "page" : undefined}>
            <Icon name={item.icon} size={17} />
            <span>{item.label}</span>
            {#if item.soon}<span class="soon">soon</span>{/if}
          </button>
        {/each}
      {/each}
    </nav>

    <div class="foot">
      <button class="nav" class:active={app.page === "settings"} onclick={() => (app.page = "settings")}>
        <Icon name="settings" size={17} /><span>Settings</span>
      </button>
      {#if app.summary}
        <div class="host">
          <div class="h">{app.summary.hostname}</div>
          <div class="muted">{app.summary.osName} {app.summary.osVersion.replace(/ \(.*\)$/, "")}</div>
          <div class="muted">Up {uptime}</div>
        </div>
      {/if}
      {#if !isTauri && !screenshotMode}<div class="demo">Demo data (browser mode)</div>{/if}
    </div>
  </aside>

  <main>
    {#if app.page === "dashboard"}<Dashboard />
    {:else if app.page === "performance"}<Performance />
    {:else if app.page === "gpu"}<Gpu />
    {:else if app.page === "processes"}<Processes />
    {:else if app.page === "hardware"}<Hardware />
    {:else if app.page === "settings"}<Settings />
    {:else if app.page === "software"}<Software />
    {:else if app.page === "cleaner"}<Cleaner />
    {:else if app.page === "largefiles"}<LargeFiles />
    {:else if app.page === "search"}<Search />
    {:else if app.page === "services"}<Services />
    {:else if app.page === "startup"}<Startup />
    {:else if app.page === "connections"}<Connections />
    {:else if app.page === "netsettings"}<NetworkSettings />
    {:else if app.page === "speedtest"}<SpeedTest />
    {:else if app.page === "nettools"}<NetTools />
    {:else if app.page === "packetforge"}<PacketForge />
    {:else if app.page === "netlens"}<NetLens />
    {:else if app.page === "launchers"}<Launchers />
    {:else if app.page === "recorder" && FEATURES.recorder}<Recorder />
    {:else if SOON[app.page]}<ComingSoon {...SOON[app.page]} />
    {/if}
  </main>
</div>

<Toasts />

<style>
  .shell { display: grid; grid-template-columns: 228px 1fr; height: 100vh; }
  aside {
    display: flex; flex-direction: column; gap: 4px; padding: 16px 12px;
    background: var(--surface-1);
    border-right: 1px solid var(--border);
    overflow-y: auto;
  }
  .brand { display: flex; align-items: center; gap: 10px; padding: 4px 8px 14px; }
  .logo {
    width: 34px; height: 34px; border-radius: 10px; display: grid; place-items: center;
    background: linear-gradient(160deg, #3987e5, #6b4fd8);
    box-shadow: 0 4px 16px rgba(57, 135, 229, 0.45);
  }
  .name { font-weight: 700; font-size: 16px; letter-spacing: -0.01em; }
  nav { display: flex; flex-direction: column; gap: 2px; flex: 1; }
  .group { font-size: 10.5px; text-transform: uppercase; letter-spacing: 0.1em; color: var(--text-muted); padding: 14px 10px 6px; }
  .nav {
    display: flex; align-items: center; gap: 10px; width: 100%; text-align: left;
    background: transparent; border: 1px solid transparent; color: var(--text-secondary);
    padding: 8px 10px; border-radius: 9px; font-size: 13.5px;
  }
  .nav:hover { background: var(--surface-2); color: var(--text-primary); }
  .nav.active {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    border-color: color-mix(in srgb, var(--accent) 30%, transparent);
    color: var(--text-primary);
  }
  .nav.active :global(svg) { color: var(--accent); }
  .soon { margin-left: auto; font-size: 10px; color: var(--text-muted); border: 1px solid var(--border-strong); border-radius: 99px; padding: 0 6px; }
  .foot { display: flex; flex-direction: column; gap: 10px; padding-top: 10px; border-top: 1px solid var(--border); }
  .host { padding: 0 10px; font-size: 11.5px; line-height: 1.5; }
  .host .h { font-weight: 600; font-size: 12.5px; }
  .demo { font-size: 11px; color: var(--warning); padding: 0 10px; }
  main { overflow-y: auto; padding: 22px 26px 32px; min-width: 0; }
</style>
