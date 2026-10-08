<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/pages/Settings.svelte
  Author : Praneeth
  License: MIT
  Purpose: User preferences (refresh rate, history, theme) and About.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import Card from "../lib/components/Card.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import { api } from "../lib/api";
  import { app } from "../lib/stores.svelte";
  import type { AlertConfig } from "../lib/types";

  const s = app.settings;
  const save = () => app.saveSettings();

  let alerts = $state<AlertConfig | null>(null);
  onMount(async () => {
    alerts = await api.getAlertConfig().catch(() => null);
  });
  async function saveAlerts() {
    if (!alerts) return;
    try {
      await api.setAlertConfig($state.snapshot(alerts));
    } catch (e) {
      app.notify(String(e), "error");
    }
  }
</script>

<PageHeader title="Settings" subtitle="Preferences are stored on this computer" />

<div class="grid">
  <Card title="Monitoring" icon="cpu">
    <div class="rows">
      <label>
        <span><b>Refresh interval</b><small>How often metrics are sampled</small></span>
        <select bind:value={s.refreshMs} onchange={save}>
          <option value={500}>0.5 s</option>
          <option value={1000}>1 s</option>
          <option value={2000}>2 s</option>
          <option value={5000}>5 s</option>
        </select>
      </label>
      <label>
        <span><b>Chart history</b><small>Number of samples kept for graphs</small></span>
        <select bind:value={s.historyLength} onchange={save}>
          <option value={60}>60 samples</option>
          <option value={120}>120 samples</option>
          <option value={300}>300 samples</option>
          <option value={600}>600 samples</option>
        </select>
      </label>
      <label>
        <span><b>Process CPU as share of whole machine</b><small>Off: 100% = one full core (like top). On: 100% = all cores.</small></span>
        <input type="checkbox" bind:checked={s.normalizeProcessCpu} onchange={save} />
      </label>
    </div>
  </Card>

  <Card title="Alerts" icon="alert">
    {#if alerts}
      <div class="rows">
        <label>
          <span><b>Desktop notifications</b><small>Warn when something needs attention, even with SysPulse in the background</small></span>
          <input type="checkbox" bind:checked={alerts.enabled} onchange={saveAlerts} />
        </label>
        <div class="thresholds" class:dim={!alerts.enabled}>
          <label><span>CPU temperature</span><input type="number" min="50" max="110" bind:value={alerts.cpuTemp} onchange={saveAlerts} /><em>°C</em></label>
          <label><span>GPU temperature</span><input type="number" min="50" max="110" bind:value={alerts.gpuTemp} onchange={saveAlerts} /><em>°C</em></label>
          <label><span>Memory in use</span><input type="number" min="50" max="100" bind:value={alerts.memoryPercent} onchange={saveAlerts} /><em>%</em></label>
          <label><span>Disk full</span><input type="number" min="50" max="100" bind:value={alerts.diskPercent} onchange={saveAlerts} /><em>%</em></label>
          <label><span>Battery below</span><input type="number" min="1" max="50" bind:value={alerts.batteryPercent} onchange={saveAlerts} /><em>%</em></label>
          <label><span>Must last for</span><input type="number" min="0" max="600" bind:value={alerts.sustainSecs} onchange={saveAlerts} /><em>s</em></label>
          <label><span>Repeat at most every</span><input type="number" min="1" max="1440" value={Math.round(alerts.cooldownSecs / 60)}
            onchange={(e) => { alerts!.cooldownSecs = Number((e.currentTarget as HTMLInputElement).value) * 60; saveAlerts(); }} /><em>min</em></label>
        </div>
      </div>
    {:else}
      <p class="muted">Alert settings are available in the desktop app.</p>
    {/if}
  </Card>

  <Card title="Appearance" icon="sun">
    <div class="rows">
      <label>
        <span><b>Theme</b><small>Follow the system or choose one</small></span>
        <select bind:value={s.theme} onchange={save}>
          <option value="dark">Dark</option>
          <option value="light">Light</option>
          <option value="system">System</option>
        </select>
      </label>
    </div>
  </Card>

  <Card title="About" icon="info">
    <div class="about">
      <div class="big">SysPulse <span class="muted">v0.6.0</span></div>
      <p class="secondary">A multipurpose system monitor and utility suite for Ubuntu.</p>
      <dl>
        <dt>Author</dt><dd>Praneeth</dd>
        <dt>License</dt><dd>MIT (open source)</dd>
        <dt>Built with</dt><dd>Rust · Tauri 2 · Svelte 5 · uPlot</dd>
        <dt>Data</dt><dd>Everything stays on this computer; nothing is uploaded</dd>
        {#if app.summary}
          <dt>System</dt><dd>{app.summary.osName} {app.summary.osVersion} · {app.summary.arch}</dd>
          <dt>Session</dt><dd>{app.summary.desktop ?? "?"} on {app.summary.sessionType ?? "?"}</dd>
        {/if}
      </dl>
    </div>
  </Card>
</div>

<style>
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(420px, 1fr)); gap: 14px; }
  .rows { display: flex; flex-direction: column; gap: 16px; }
  label { display: flex; justify-content: space-between; align-items: center; gap: 16px; }
  label span { display: flex; flex-direction: column; gap: 2px; }
  small { color: var(--text-muted); font-size: 12px; }
  input[type="checkbox"] { width: 18px; height: 18px; accent-color: var(--accent); }
  .thresholds { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px 18px; }
  .thresholds.dim { opacity: 0.45; pointer-events: none; }
  .thresholds label { justify-content: flex-start; gap: 8px; font-size: 13px; color: var(--text-secondary); }
  .thresholds span { flex: 1; }
  .thresholds input { width: 72px; text-align: right; }
  .thresholds em { font-style: normal; color: var(--text-muted); width: 26px; }
  .big { font-size: 20px; font-weight: 700; }
  .about p { margin: 6px 0 12px; }
  dl { display: grid; grid-template-columns: 90px 1fr; gap: 6px 12px; margin: 0; font-size: 13px; }
  dt { color: var(--text-muted); }
  dd { margin: 0; }
</style>
