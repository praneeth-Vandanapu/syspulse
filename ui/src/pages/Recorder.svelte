<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/pages/Recorder.svelte
  Author : Praneeth
  License: MIT
  Purpose: Screen recorder: choose what to capture (monitor / window /
           region), audio (system sound, microphone, levels, volume),
           format and quality; start with a countdown; control from here,
           the top-bar menu or global hotkeys; browse recordings.
-->
<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import Icon from "../lib/components/Icon.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import { api, isTauri, onEvent, pickFolder } from "../lib/api";
  import { app } from "../lib/stores.svelte";
  import { bytes, duration } from "../lib/format";
  import { FOLLOW_INPUT, FOLLOW_OUTPUT, rec, type AudioDevice, type AudioState, type CaptureArea, type RecorderEvent, type RecordingFile, type RecordingResult, type Rect, type Sources, type StartOptions, type Status } from "../lib/recorder";

  let sources = $state<Sources | null>(null);
  let opts = $state<StartOptions | null>(null);
  let status = $state<Status>({ state: "idle", elapsed: 0, size: 0, info: null });
  let countdown = $state(0);
  let levels = $state<{ system: number | null; mic: number | null }>({ system: null, mic: null });
  let files = $state<RecordingFile[]>([]);
  let last = $state<RecordingResult | null>(null);
  let areaKind = $state<"monitor" | "window" | "region">("monitor");
  let useSystem = $state(true);
  let useMic = $state(false);
  let loading = $state(true);
  const unlisten: (() => void)[] = [];

  async function load() {
    loading = true;
    try {
      const [src, defaults, st] = await Promise.all([rec.sources(), rec.defaults(), rec.status()]);
      // Derive every field from the loaded settings *before* assigning `opts`:
      // the sync effect below writes opts.config.audio from these fields.
      areaKind = defaults.config.area.kind;
      useSystem = !!defaults.config.audio.system;
      useMic = !!defaults.config.audio.mic;
      // Default to following the current output/input. Migrate older settings
      // that pinned a device: a pin equal to today's default, or a device with
      // nothing plugged in, becomes "follow" (it was what the user meant).
      const find = (n: string | null) => src.audio.find((a) => a.name === n);
      const s0 = defaults.config.audio.system;
      const m0 = defaults.config.audio.mic;
      sysDev = !s0 || find(s0)?.isDefault || find(s0)?.available === false ? FOLLOW_OUTPUT : s0;
      micDev = !m0 || find(m0)?.isDefault || find(m0)?.available === false ? FOLLOW_INPUT : m0;
      sources = src;
      status = st;
      opts = defaults;
      refreshFiles();
      refreshAudio();
    } catch (e) {
      app.notify(String(e), "error");
    } finally {
      loading = false;
    }
  }
  let sysDev = $state<string | null>(FOLLOW_OUTPUT);
  let micDev = $state<string | null>(FOLLOW_INPUT);
  let audio = $state<AudioState | null>(null);

  // Bluetooth headsets and USB devices come and go: refresh every 3 s.
  async function refreshAudio() {
    if (status.state !== "idle" && audio) return;
    audio = await rec.audio().catch(() => audio);
  }
  const audioTimer = setInterval(refreshAudio, 3000);
  onDestroy(() => clearInterval(audioTimer));

  const devName = (d: AudioDevice) => d.description.replace(/^Monitor of /, "");
  const devLabel = (d: AudioDevice) => `${devName(d)}${d.isBluetooth ? " (Bluetooth)" : ""}${d.available ? "" : " — not plugged in"}`;
  function picked(value: string | null, output: boolean): AudioDevice | null {
    if (!audio || !value) return null;
    if (value === (output ? FOLLOW_OUTPUT : FOLLOW_INPUT)) return output ? audio.currentOutput : audio.currentInput;
    return audio.devices.find((d) => d.name === value) ?? null;
  }
  let sysPicked = $derived(picked(sysDev, true));
  let micPicked = $derived(picked(micDev, false));
  let sysWarn = $derived.by(() => {
    if (!useSystem || !audio || !sysPicked) return "";
    if (!sysPicked.available) return `${devName(sysPicked)} is not connected — the recording will be silent.`;
    if (sysDev !== FOLLOW_OUTPUT && audio.currentOutput && sysPicked.name !== audio.currentOutput.name)
      return `Sound is playing through ${devName(audio.currentOutput)}, but this records ${devName(sysPicked)}. Choose “Follow current output”.`;
    return "";
  });
  let micWarn = $derived.by(() => {
    if (!useMic || !micPicked) return "";
    if (!micPicked.available) return `Nothing is plugged into ${devName(micPicked)} — it records only background hiss.`;
    if (micPicked.isBluetooth) return "Using the earbuds' microphone switches them to headset mode (lower sound quality) while recording.";
    return "";
  });

  async function refreshFiles() {
    if (opts) files = await rec.list(opts.config.outputDir).catch(() => []);
  }

  onMount(async () => {
    await load();
    if (!isTauri) return;
    unlisten.push(await onEvent<Status>("recorder-status", (s) => (status = s)));
    unlisten.push(await onEvent<number>("recorder-countdown", (n) => (countdown = n)));
    unlisten.push(await onEvent<RecorderEvent>("recorder-event", (e) => {
      if (e.type === "level") levels = { system: e.system, mic: e.mic };
    }));
    unlisten.push(await onEvent<RecordingResult>("recorder-finished", (r) => {
      last = r;
      status = { state: "idle", elapsed: 0, size: 0, info: null };
      refreshFiles();
      app.notify(`Saved ${r.path.split("/").pop()} (${duration(r.durationSecs)}, ${bytes(r.size)})`);
      restartMeter();
    }));
    unlisten.push(await onEvent<string>("recorder-error", (m) => {
      app.notify(m, "error", 10000);
      status = { state: "idle", elapsed: 0, size: 0, info: null };
      restartMeter();
    }));
    unlisten.push(await onEvent("recorder-cancelled", () => {
      app.notify("Recording cancelled and discarded", "info");
      restartMeter();
    }));
    unlisten.push(await onEvent<Rect | null>("region-selected", (r) => {
      if (r && opts) {
        opts.config.area = { kind: "region", ...r };
        areaKind = "region";
      }
    }));
  });
  onDestroy(() => {
    unlisten.forEach((u) => u());
    if (isTauri && status.state === "idle") rec.meter(null, null).catch(() => {});
  });

  // Keep config in sync with the toggles and persist it for tray/hotkeys.
  $effect(() => {
    if (!opts) return;
    opts.config.audio.system = useSystem ? sysDev : null;
    opts.config.audio.mic = useMic ? micDev : null;
    if (opts.config.video.container === "webm") opts.config.video.codec = "vp8";
    else if (opts.config.video.codec === "vp8") opts.config.video.codec = "h264";
    const snap = $state.snapshot(opts) as StartOptions;
    rec.saveDefaults(snap).catch(() => {});
  });

  // Live level meter while idle.
  let meterKey = "";
  function restartMeter() {
    if (!isTauri || !opts || status.state !== "idle") return;
    meterKey = "";
    applyMeter();
  }
  function applyMeter() {
    const key = `${useSystem ? sysDev : ""}|${useMic ? micDev : ""}`;
    if (key === meterKey) return;
    meterKey = key;
    levels = { system: null, mic: null };
    rec.meter(useSystem ? sysDev : null, useMic ? micDev : null).catch(() => {});
  }
  $effect(() => {
    void [useSystem, useMic, sysDev, micDev, status.state];
    if (opts && status.state === "idle") applyMeter();
  });

  function chooseMonitor(i: number) {
    if (!opts || !sources) return;
    const m = sources.monitors[i];
    opts.config.area = { kind: "monitor", name: m.name, x: m.x, y: m.y, width: m.width, height: m.height };
  }
  function chooseWindow(xid: string) {
    if (!opts || !sources) return;
    const w = sources.windows.find((x) => x.xid === xid);
    if (w) opts.config.area = { kind: "window", xid: w.xid, title: w.title, width: w.width, height: w.height };
  }
  function setKind(k: typeof areaKind) {
    areaKind = k;
    if (!opts || !sources) return;
    if (k === "monitor" && opts.config.area.kind !== "monitor") chooseMonitor(0);
    if (k === "window" && opts.config.area.kind !== "window" && sources.windows.length) chooseWindow(sources.windows[0].xid);
  }
  async function pickRegion() {
    if (!sources) return;
    const area = opts?.config.area;
    const m = sources.monitors.find((x) => area && "name" in area && x.name === area.name) ?? sources.monitors[0];
    try {
      await rec.pickRegion({ x: m.x, y: m.y, width: m.width, height: m.height });
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  async function start() {
    if (!opts) return;
    if (areaKind === "region" && opts.config.area.kind !== "region") {
      app.notify("Select a region first", "info");
      return;
    }
    try {
      await rec.meter(null, null);
      meterKey = "";
      await rec.start($state.snapshot(opts) as StartOptions);
      if (opts.countdown === 0) status = { ...status, state: "recording" };
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  async function browse() {
    const p = await pickFolder();
    if (p && opts) {
      opts.config.outputDir = p;
      refreshFiles();
    }
  }

  async function remove(f: RecordingFile) {
    const r = await api.filesTrash([f.path]).catch(() => null);
    if (r?.done.length) {
      files = files.filter((x) => x.path !== f.path);
      app.notify(`Moved ${f.name} to Trash`);
    }
  }

  const db = (v: number | null) => (v == null ? 0 : Math.max(0, Math.min(100, ((v + 60) / 60) * 100)));
  let active = $derived(status.state === "recording" || status.state === "paused");
  let hwEncoder = $derived(sources?.encoders.some((e) => e.hardware) ?? false);
  let codecEncoders = $derived(sources?.encoders.filter((e) => e.codec === (opts?.config.video.codec ?? "h264")) ?? []);
  const areaLabel = (a: CaptureArea) =>
    a.kind === "monitor" ? `${a.name} (${a.width}×${a.height})` : a.kind === "window" ? `“${a.title}” (${a.width}×${a.height})` : `${a.width}×${a.height} at ${a.x},${a.y}`;
</script>

<PageHeader title="Screen Recorder" subtitle="Record the screen, a window or a region — with system sound and microphone">
  {#snippet actions()}
    <button onclick={load} disabled={loading || active}><Icon name="refresh" size={14} /> Refresh sources</button>
  {/snippet}
</PageHeader>

{#if sources && !sources.supported}
  <div class="banner"><Icon name="alert" size={15} /> This session is <b>{sources.sessionType}</b>. This version records X11 sessions; Wayland support (via the screen-sharing portal) is planned. Choose "Ubuntu on Xorg" at the login screen to record now.</div>
{:else if sources && opts}
  {@const o = opts}
  {#if sources.missing.length}
    <div class="banner"><Icon name="alert" size={15} /> Missing components: {sources.missing.join(", ")}</div>
  {/if}

  <section class="hero" class:rec={status.state === "recording"} class:paused={status.state === "paused"}>
    <div class="state">
      <span class="lamp"></span>
      <div>
        <div class="big num">{active ? duration(status.elapsed) : status.state === "countdown" ? "Starting…" : "Ready"}</div>
        <div class="muted">
          {#if active && status.info}{status.state === "paused" ? "Paused · " : ""}{bytes(status.size)} · {status.info.width}×{status.info.height} @ {status.info.fps} fps · {status.info.encoder}{status.info.audio.length ? ` · ${status.info.audio.join(", ")}` : " · no audio"}
          {:else}{areaLabel(o.config.area)} · {o.config.video.container.toUpperCase()} · {useSystem && useMic ? "system + mic" : useSystem ? "system sound" : useMic ? "microphone" : "no audio"}{/if}
        </div>
      </div>
    </div>
    <div class="ctrl">
      {#if active}
        <button class="big-btn" onclick={() => rec.togglePause()}><Icon name={status.state === "paused" ? "play" : "stop"} size={16} /> {status.state === "paused" ? "Resume" : "Pause"}</button>
        <button class="big-btn primary" onclick={() => rec.stop(true)}><Icon name="check" size={16} /> Stop & save</button>
        <button class="big-btn ghost" onclick={() => rec.stop(false)}>Cancel</button>
      {:else if status.state === "countdown"}
        <button class="big-btn" onclick={() => rec.stop(false)}>Cancel</button>
      {:else}
        <button class="big-btn record" onclick={start} disabled={sources.missing.length > 0}><span class="dot"></span> Start recording</button>
      {/if}
    </div>
    <div class="keys muted">Hotkeys: <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>R</kbd> start/stop · <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>P</kbd> pause · controls also in the top-bar menu (●)</div>
  </section>

  <div class="grid" class:locked={active || status.state === "countdown"}>
    <section class="panel">
      <h3>What to record</h3>
      <div class="seg">
        <button class:on={areaKind === "monitor"} onclick={() => setKind("monitor")}>Full screen</button>
        <button class:on={areaKind === "window"} onclick={() => setKind("window")}>Window</button>
        <button class:on={areaKind === "region"} onclick={() => setKind("region")}>Region</button>
      </div>
      {#if areaKind === "monitor"}
        <div class="list">
          {#each sources.monitors as m, i (m.name)}
            <label class="opt"><input type="radio" name="mon" checked={o.config.area.kind === "monitor" && o.config.area.name === m.name} onchange={() => chooseMonitor(i)} />
              <b>{m.name}</b> <span class="muted">{m.width}×{m.height}{m.primary ? " · primary" : ""}</span></label>
          {/each}
        </div>
      {:else if areaKind === "window"}
        <select class="wide" value={o.config.area.kind === "window" ? o.config.area.xid : ""} onchange={(e) => chooseWindow((e.currentTarget as HTMLSelectElement).value)}>
          {#each sources.windows as w (w.xid)}<option value={w.xid}>{w.title} — {w.app} ({w.width}×{w.height})</option>{/each}
        </select>
        <p class="muted small">Records the window's contents even when other windows cover it. Keep its size constant while recording.</p>
      {:else}
        <button onclick={pickRegion}><Icon name="search" size={14} /> {o.config.area.kind === "region" ? "Select again…" : "Select region on screen…"}</button>
        {#if o.config.area.kind === "region"}
          {@const a = o.config.area}
          <div class="row4">
            <label>X <input type="number" bind:value={a.x} /></label>
            <label>Y <input type="number" bind:value={a.y} /></label>
            <label>W <input type="number" min="16" bind:value={a.width} /></label>
            <label>H <input type="number" min="16" bind:value={a.height} /></label>
          </div>
        {/if}
      {/if}
      <label class="opt"><input type="checkbox" bind:checked={o.config.video.showPointer} /> Show mouse pointer</label>
    </section>

    <section class="panel">
      <h3>Audio</h3>
      <div class="audio">
        <label class="opt"><input type="checkbox" bind:checked={useSystem} /> <b>System sound</b></label>
        <select bind:value={sysDev} disabled={!useSystem}>
          <option value={FOLLOW_OUTPUT}>Follow current output{audio?.currentOutput ? ` — ${devName(audio.currentOutput)}` : ""}</option>
          {#each (audio?.devices ?? sources.audio).filter((a) => a.isMonitor) as a (a.name)}<option value={a.name}>{devLabel(a)}</option>{/each}
        </select>
        {#if sysWarn}<p class="warn"><Icon name="alert" size={12} /> {sysWarn}</p>{/if}
        <div class="meter"><span style:width="{useSystem ? db(levels.system) : 0}%" class:hot={(levels.system ?? -99) > -3}></span></div>
        <label class="vol">Volume <input type="range" min="0" max="2" step="0.05" bind:value={o.config.audio.systemVolume} disabled={!useSystem} /> <span class="num">{Math.round(o.config.audio.systemVolume * 100)}%</span></label>
      </div>
      <div class="audio">
        <label class="opt"><input type="checkbox" bind:checked={useMic} /> <b>Microphone</b></label>
        <select bind:value={micDev} disabled={!useMic}>
          <option value={FOLLOW_INPUT}>Follow current input{audio?.currentInput ? ` — ${devName(audio.currentInput)}` : ""}</option>
          {#each (audio?.devices ?? sources.audio).filter((a) => !a.isMonitor) as a (a.name)}<option value={a.name}>{devLabel(a)}</option>{/each}
        </select>
        {#if micWarn}<p class="warn"><Icon name="alert" size={12} /> {micWarn}</p>{/if}
        <div class="meter"><span style:width="{useMic ? db(levels.mic) : 0}%" class:hot={(levels.mic ?? -99) > -3}></span></div>
        <label class="vol">Volume <input type="range" min="0" max="2" step="0.05" bind:value={o.config.audio.micVolume} disabled={!useMic} /> <span class="num">{Math.round(o.config.audio.micVolume * 100)}%</span></label>
      </div>
      <p class="muted small">Both selected = mixed into one track.</p>
    </section>

    <section class="panel">
      <h3>Format & quality</h3>
      <div class="form">
        <label>Container <select bind:value={o.config.video.container}><option value="mp4">MP4 (most compatible)</option><option value="mkv">MKV (survives crashes)</option><option value="webm">WebM (VP8 + Opus)</option></select></label>
        {#if o.config.video.container !== "webm"}
          <label>Codec <select bind:value={o.config.video.codec}><option value="h264">H.264</option><option value="h265">H.265 / HEVC (smaller files)</option></select></label>
        {/if}
        <label>Frame rate <select bind:value={o.config.video.fps}>{#each [15, 24, 30, 60] as f (f)}<option value={f}>{f} fps</option>{/each}</select></label>
        <label>Quality <select bind:value={o.config.video.quality}><option value="standard">Standard (small files)</option><option value="high">High</option><option value="max">Maximum</option></select></label>
        <label>Encoder
          <select bind:value={o.config.video.encoder}>
            <option value="auto">Automatic (best available)</option>
            {#each codecEncoders as e (e.element)}<option value={e.element}>{e.label}</option>{/each}
          </select>
        </label>
      </div>
      {#if !hwEncoder}
        <p class="muted small"><Icon name="info" size={12} /> No working GPU encoder: GStreamer 1.20 on this system cannot use the NVIDIA driver's NVENC. Encoding uses the CPU (about half a core at 1080p30).</p>
      {/if}
    </section>

    <section class="panel">
      <h3>Options</h3>
      <div class="form">
        <label>Countdown <select bind:value={o.countdown}><option value={0}>None</option><option value={3}>3 seconds</option><option value={5}>5 seconds</option><option value={10}>10 seconds</option></select></label>
        <label class="opt"><input type="checkbox" bind:checked={o.hideWindow} /> Hide SysPulse while recording (control from the top bar)</label>
        <label>Save to <input class="mono" bind:value={o.config.outputDir} onchange={refreshFiles} /><button class="small" onclick={browse}>Browse…</button></label>
      </div>
    </section>
  </div>

  <section class="panel files">
    <h3>Recordings <span class="muted small">{files.length} in {o.config.outputDir}</span></h3>
    {#each files.slice(0, 50) as f (f.path)}
      <div class="file" class:new={last?.path === f.path}>
        <Icon name="record" size={15} />
        <span class="fname">{f.name}</span>
        <span class="muted num">{bytes(f.size)}</span>
        <span class="muted num">{new Date(f.modified * 1000).toLocaleString()}</span>
        <button class="small" onclick={() => api.openPath(f.path)}>Play</button>
        <button class="small" onclick={() => api.revealPath(f.path)}>Show in folder</button>
        <button class="small ghost" onclick={() => remove(f)} aria-label="Move to Trash"><Icon name="close" size={13} /></button>
      </div>
    {:else}
      <p class="muted small">No recordings yet.</p>
    {/each}
  </section>
{:else if loading}
  <p class="muted">Detecting monitors, windows, audio devices and encoders…</p>
{/if}

{#if countdown > 0}
  <div class="count" role="status"><span>{countdown}</span><button onclick={() => rec.stop(false)}>Cancel</button></div>
{/if}

<style>
  button :global(svg) { vertical-align: -2px; }
  .banner { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; font-size: 13px; padding: 10px 14px; border-radius: 10px; margin-bottom: 14px;
    background: color-mix(in srgb, var(--warning) 12%, transparent); border: 1px solid color-mix(in srgb, var(--warning) 35%, transparent); }
  .hero { display: grid; grid-template-columns: 1fr auto; gap: 10px 20px; align-items: center; background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); padding: 18px 22px; margin-bottom: 14px; }
  .hero.rec { border-color: color-mix(in srgb, var(--critical) 60%, transparent); }
  .hero.paused { border-color: color-mix(in srgb, var(--warning) 60%, transparent); }
  .state { display: flex; gap: 16px; align-items: center; }
  .lamp { width: 16px; height: 16px; border-radius: 50%; background: var(--surface-3); flex: none; }
  .rec .lamp { background: var(--critical); }
  .paused .lamp { background: var(--warning); }
  .big { font-size: 28px; font-weight: 700; letter-spacing: -0.01em; }
  .ctrl { display: flex; gap: 8px; }
  .big-btn { padding: 10px 18px; font-size: 14px; display: inline-flex; gap: 8px; align-items: center; }
  .big-btn.record { background: var(--critical); color: #fff; border-color: transparent; font-weight: 600; }
  .big-btn .dot { width: 12px; height: 12px; border-radius: 50%; background: #fff; }
  .keys { grid-column: 1 / -1; font-size: 12px; }
  kbd { font-family: var(--font-mono); font-size: 11px; border: 1px solid var(--border-strong); border-radius: 4px; padding: 0 4px; background: var(--surface-2); }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(380px, 1fr)); gap: 14px; margin-bottom: 14px; }
  .grid.locked { opacity: 0.55; pointer-events: none; }
  .panel { background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); padding: 14px 16px; display: flex; flex-direction: column; gap: 10px; }
  .panel h3 { font-size: 12px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--text-secondary); display: flex; gap: 10px; align-items: baseline; }
  .seg { display: inline-flex; align-self: flex-start; background: var(--surface-2); border: 1px solid var(--border); border-radius: 10px; padding: 3px; gap: 2px; }
  .seg button { border: none; background: transparent; padding: 5px 14px; border-radius: 7px; color: var(--text-secondary); font-size: 13px; }
  .seg button.on { background: var(--surface-3); color: var(--text-primary); }
  .list { display: flex; flex-direction: column; gap: 6px; }
  .opt { display: flex; gap: 8px; align-items: center; font-size: 13px; }
  .opt input { accent-color: var(--accent); }
  .wide { width: 100%; }
  .small { font-size: 12px; padding: 3px 9px; }
  p.small { margin: 0; }
  .row4 { display: grid; grid-template-columns: repeat(4, 1fr); gap: 8px; }
  .row4 label { display: flex; gap: 6px; align-items: center; font-size: 12.5px; color: var(--text-secondary); }
  .row4 input { width: 100%; }
  .audio { display: grid; grid-template-columns: 150px 1fr; gap: 6px 12px; align-items: center; padding-bottom: 8px; border-bottom: 1px solid var(--border); }
  .audio select { min-width: 0; }
  .warn { grid-column: 1 / -1; margin: 0; font-size: 12px; color: var(--warning); display: flex; gap: 5px; align-items: flex-start; }
  .meter { grid-column: 1 / -1; height: 8px; border-radius: 6px; background: var(--surface-3); overflow: hidden; }
  .meter span { display: block; height: 100%; background: var(--good); border-radius: 6px; }
  .meter span.hot { background: var(--critical); }
  .vol { grid-column: 1 / -1; display: flex; gap: 10px; align-items: center; font-size: 12.5px; color: var(--text-secondary); }
  .vol input { flex: 1; accent-color: var(--accent); }
  .form { display: flex; flex-direction: column; gap: 10px; }
  .form > label:not(.opt) { display: flex; gap: 10px; align-items: center; font-size: 13px; color: var(--text-secondary); }
  .form select, .form input:not([type="checkbox"]) { flex: 1; }
  .files { margin-bottom: 20px; }
  .file { display: grid; grid-template-columns: 18px minmax(0, 1fr) 90px 170px auto auto auto; gap: 10px; align-items: center; padding: 6px 0; border-top: 1px solid var(--border); font-size: 13px; }
  .file.new .fname { color: var(--good); font-weight: 600; }
  .fname { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .count { position: fixed; inset: 0; z-index: 80; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 20px; background: rgba(0, 0, 0, 0.6); }
  .count span { font-size: 160px; font-weight: 800; color: #fff; line-height: 1; }
</style>
