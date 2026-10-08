<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/pages/NetworkSettings.svelte
  Author : Praneeth
  License: MIT
  Purpose: Network settings via NetworkManager: adapters with live addressing,
           IPv4/IPv6 editor (DHCP / static, multiple addresses, gateway, DNS,
           MTU), Wi-Fi scanner with channel congestion, saved profiles.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "../lib/components/Icon.svelte";
  import Modal from "../lib/components/Modal.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import { api } from "../lib/api";
  import { app } from "../lib/stores.svelte";
  import type { ConnectionSettings, IpSettings, NetConnection, NetDevice, WifiNetwork } from "../lib/types";

  let tab = $state<"adapters" | "wifi" | "profiles">("adapters");
  let devices = $state<NetDevice[]>([]);
  let profiles = $state<NetConnection[]>([]);
  let wifi = $state<WifiNetwork[]>([]);
  let radio = $state(true);
  let loading = $state(true);
  let scanning = $state(false);

  // Editor
  let edit = $state<ConnectionSettings | null>(null);
  let addr4 = $state<{ ip: string; prefix: number }[]>([]);
  let addr6 = $state<{ ip: string; prefix: number }[]>([]);
  let dns4 = $state("");
  let dns6 = $state("");
  let mtuAuto = $state(true);
  let saving = $state(false);
  let confirmApply = $state(false);

  async function load() {
    loading = true;
    try {
      [devices, profiles] = await Promise.all([api.netDevices(), api.netConnections()]);
    } catch (e) {
      app.notify(String(e), "error");
    } finally {
      loading = false;
    }
  }
  async function loadWifi(rescan = false) {
    scanning = true;
    try {
      radio = await api.wifiRadio(null);
      wifi = radio ? await api.wifiScan(rescan) : [];
    } catch (e) {
      app.notify(String(e), "error");
    } finally {
      scanning = false;
    }
  }
  onMount(() => {
    load();
    loadWifi(false);
  });

  const split = (cidr: string) => {
    const [ip, p] = cidr.split("/");
    return { ip, prefix: Number(p) || 24 };
  };

  async function openEditor(uuid: string) {
    try {
      const s = await api.netConnectionSettings(uuid);
      edit = s;
      addr4 = s.ipv4.addresses.map(split);
      addr6 = s.ipv6.addresses.map((a) => ({ ...split(a), prefix: Number(a.split("/")[1]) || 64 }));
      dns4 = s.ipv4.dns.join(", ");
      dns6 = s.ipv6.dns.join(", ");
      mtuAuto = s.mtu == null;
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  const V4 = /^(25[0-5]|2[0-4]\d|1?\d?\d)(\.(25[0-5]|2[0-4]\d|1?\d?\d)){3}$/;
  const isV4 = (s: string) => V4.test(s.trim());
  const isV6 = (s: string) => s.includes(":") && /^[0-9a-fA-F:.]+$/.test(s.trim());
  const list = (s: string) => s.split(/[,\s]+/).map((x) => x.trim()).filter(Boolean);
  const mask = (p: number) => {
    const m = p === 0 ? 0 : (0xffffffff << (32 - p)) >>> 0;
    return [24, 16, 8, 0].map((s) => (m >>> s) & 255).join(".");
  };

  let errors = $derived.by(() => {
    if (!edit) return [];
    const e: string[] = [];
    if (edit.ipv4.method === "manual") {
      if (!addr4.length) e.push("IPv4: add at least one address");
      addr4.forEach((a, i) => !isV4(a.ip) && e.push(`IPv4 address ${i + 1} is not valid`));
      if (edit.ipv4.gateway && !isV4(edit.ipv4.gateway)) e.push("IPv4 gateway is not valid");
    }
    list(dns4).forEach((d) => !isV4(d) && e.push(`DNS "${d}" is not an IPv4 address`));
    if (edit.ipv6.method === "manual") {
      if (!addr6.length) e.push("IPv6: add at least one address");
      addr6.forEach((a, i) => !isV6(a.ip) && e.push(`IPv6 address ${i + 1} is not valid`));
    }
    list(dns6).forEach((d) => !isV6(d) && e.push(`DNS "${d}" is not an IPv6 address`));
    if (!mtuAuto && (!edit.mtu || edit.mtu < 576 || edit.mtu > 9216)) e.push("MTU must be 576–9216");
    return e;
  });

  function build(): ConnectionSettings {
    const s = $state.snapshot(edit!) as ConnectionSettings;
    const ip4: IpSettings = { ...s.ipv4, addresses: s.ipv4.method === "manual" ? addr4.map((a) => `${a.ip.trim()}/${a.prefix}`) : [], dns: list(dns4), gateway: s.ipv4.method === "manual" ? s.ipv4.gateway?.trim() || null : null };
    const ip6: IpSettings = { ...s.ipv6, addresses: s.ipv6.method === "manual" ? addr6.map((a) => `${a.ip.trim()}/${a.prefix}`) : [], dns: list(dns6), gateway: s.ipv6.method === "manual" ? s.ipv6.gateway?.trim() || null : null };
    return { ...s, ipv4: ip4, ipv6: ip6, mtu: mtuAuto ? null : s.mtu };
  }

  async function save(apply: boolean) {
    if (!edit || errors.length) return;
    saving = true;
    confirmApply = false;
    try {
      const msg = await api.netUpdateConnection(build(), apply);
      app.notify(msg);
      edit = null;
      setTimeout(load, apply ? 2500 : 300);
    } catch (e) {
      app.notify(String(e).includes("permission") ? "NetworkManager refused the change (administrator rights needed)" : String(e), "error", 9000);
    } finally {
      saving = false;
    }
  }

  async function toggle(uuid: string | null, up: boolean) {
    if (!uuid) return;
    try {
      await api.netActivate(uuid, up);
      app.notify(up ? "Connecting…" : "Disconnected");
      setTimeout(load, 1500);
      if (tab === "wifi") setTimeout(() => loadWifi(false), 2500);
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  async function setRadio(on: boolean) {
    try {
      radio = await api.wifiRadio(on);
      setTimeout(() => loadWifi(true), 1500);
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  const KIND: Record<string, string> = { "802-3-ethernet": "Ethernet", "802-11-wireless": "Wi-Fi", bridge: "Bridge", vpn: "VPN", wireguard: "WireGuard", loopback: "Loopback" };
  const kindLabel = (k: string) => KIND[k] ?? k;
  const bars = (sig: number) => (sig >= 75 ? 4 : sig >= 55 ? 3 : sig >= 35 ? 2 : 1);

  // Channel congestion: networks per channel, per band.
  let channels = $derived.by(() => {
    const by = (band: string) => {
      const m = new Map<number, WifiNetwork[]>();
      for (const w of wifi.filter((x) => x.band === band)) {
        if (!m.has(w.channel)) m.set(w.channel, []);
        m.get(w.channel)!.push(w);
      }
      return [...m.entries()].sort((a, b) => a[0] - b[0]);
    };
    return [
      { band: "2.4 GHz", items: by("2.4 GHz") },
      { band: "5 GHz", items: by("5 GHz") },
      { band: "6 GHz", items: by("6 GHz") },
    ].filter((b) => b.items.length);
  });
  let maxPerChannel = $derived(Math.max(1, ...channels.flatMap((b) => b.items.map(([, l]) => l.length))));
</script>

<PageHeader title="Network Settings" subtitle="Adapters, IP addressing and Wi-Fi (via NetworkManager)">
  {#snippet actions()}
    <div class="seg">
      <button class:on={tab === "adapters"} onclick={() => (tab = "adapters")}>Adapters</button>
      <button class:on={tab === "wifi"} onclick={() => { tab = "wifi"; loadWifi(false); }}>Wi-Fi networks</button>
      <button class:on={tab === "profiles"} onclick={() => (tab = "profiles")}>Saved connections</button>
    </div>
    <button onclick={() => { load(); loadWifi(false); }} disabled={loading}><Icon name="refresh" size={14} /> Refresh</button>
  {/snippet}
</PageHeader>

{#if tab === "adapters"}
  <div class="cards">
    {#each devices as d (d.device)}
      {@const up = d.state.startsWith("connected")}
      <article class="dev">
        <header>
          <span class="ic" class:up><Icon name={d.kind === "wifi" ? "network" : "usb"} size={18} /></span>
          <div class="tt">
            <h3>{d.connection ?? d.device}</h3>
            <div class="muted">{d.device} · {kindLabel(d.kind === "ethernet" ? "802-3-ethernet" : d.kind === "wifi" ? "802-11-wireless" : d.kind)}{d.driver ? ` · ${d.driver}` : ""}</div>
          </div>
          <span class="pill" class:good={up}>{d.state.replace(" (externally)", "")}</span>
        </header>
        {#if d.wifi}
          <div class="wifi">
            <span class="sig">{#each [1, 2, 3, 4] as b (b)}<i class:on={bars(d.wifi.signal ?? 0) >= b} style:height="{b * 4 + 2}px"></i>{/each}</span>
            <b>{d.wifi.ssid}</b> <span class="muted">{d.wifi.signal}% · {d.wifi.bitrate} · {d.wifi.frequencyMhz ? (d.wifi.frequencyMhz / 1000).toFixed(1) + " GHz" : ""} · {d.wifi.security}</span>
          </div>
        {/if}
        <dl>
          <dt>IPv4</dt><dd class="mono">{d.ipv4.length ? d.ipv4.join("\n") : "—"}</dd>
          <dt>Gateway</dt><dd class="mono">{d.ipv4Gateway ?? "—"}</dd>
          <dt>DNS</dt><dd class="mono">{d.ipv4Dns.join(", ") || "—"}</dd>
          {#if d.ipv6.length}<dt>IPv6</dt><dd class="mono">{d.ipv6.join("\n")}</dd>{/if}
          <dt>MAC</dt><dd class="mono">{d.mac ?? "—"}</dd>
          <dt>Link</dt><dd>{d.speedMbps ? `${d.speedMbps >= 1000 ? d.speedMbps / 1000 + " Gbit/s" : d.speedMbps + " Mbit/s"}` : d.carrier === false ? "Cable unplugged" : "—"}{d.mtu ? ` · MTU ${d.mtu}` : ""}</dd>
        </dl>
        <footer>
          {#if d.connectionUuid && d.kind !== "bridge"}
            <button class="primary small" onclick={() => openEditor(d.connectionUuid!)}><Icon name="settings" size={13} /> Edit IP settings</button>
            <button class="small" onclick={() => toggle(d.connectionUuid, false)}>Disconnect</button>
          {:else if d.kind === "bridge"}
            <span class="muted small">Managed by another program (e.g. Docker)</span>
          {:else}
            <span class="muted small">Not connected — activate a saved connection under "Saved connections"</span>
          {/if}
        </footer>
      </article>
    {:else}
      {#if !loading}<div class="muted">No network adapters found (is NetworkManager running?).</div>{/if}
    {/each}
  </div>
{:else if tab === "wifi"}
  <div class="wbar">
    <label class="radio"><input type="checkbox" checked={radio} onchange={(e) => setRadio((e.currentTarget as HTMLInputElement).checked)} /> Wi-Fi radio {radio ? "on" : "off"}</label>
    <span class="grow"></span>
    <span class="muted small">{wifi.length} networks</span>
    <button onclick={() => loadWifi(true)} disabled={scanning || !radio}><Icon name="refresh" size={14} /> {scanning ? "Scanning…" : "Rescan"}</button>
  </div>
  {#if channels.length}
    <section class="chan">
      {#each channels as b (b.band)}
        <div class="band">
          <div class="muted small">{b.band} — networks per channel</div>
          <div class="bars">
            {#each b.items as [ch, nets] (ch)}
              <div class="col" title="Channel {ch}: {nets.map((n) => n.ssid || '(hidden)').join(', ')}">
                <span class="bar" class:mine={nets.some((n) => n.inUse)} style:height="{(nets.length / maxPerChannel) * 60 + 6}px"></span>
                <span class="cn num">{ch}</span>
              </div>
            {/each}
          </div>
        </div>
      {/each}
    </section>
  {/if}
  <div class="table">
    {#each wifi as w (w.bssid)}
      <div class="wrow" class:cur={w.inUse}>
        <span class="sig">{#each [1, 2, 3, 4] as b (b)}<i class:on={bars(w.signal) >= b} style:height="{b * 4 + 2}px"></i>{/each}</span>
        <div class="meta"><b>{w.ssid || "(hidden network)"}</b> <span class="muted mono small">{w.bssid}</span></div>
        <span class="num">{w.signal}%</span>
        <span class="muted small">ch {w.channel} · {w.band}</span>
        <span class="muted small">{w.rate}</span>
        <span class="sec" class:open={w.security === "Open"}>{w.security}</span>
        {#if w.inUse}
          <span class="pill good">Connected</span>
        {:else if w.knownUuid}
          <button class="small" onclick={() => toggle(w.knownUuid, true)}>Connect</button>
        {:else}
          <span class="muted small" title="Connect to new networks from Ubuntu Settings">not saved</span>
        {/if}
      </div>
    {:else}
      <div class="muted pad">{radio ? (scanning ? "Scanning…" : "No networks found.") : "Wi-Fi is turned off."}</div>
    {/each}
  </div>
{:else}
  <div class="table">
    {#each profiles as p (p.uuid)}
      <div class="prow">
        <span class="dot" class:on={p.active}></span>
        <div class="meta"><b>{p.name}</b> <span class="muted small">{kindLabel(p.kind)}{p.device ? ` · ${p.device}` : ""}{p.autoconnect ? " · connects automatically" : ""}</span></div>
        {#if p.kind === "802-3-ethernet" || p.kind === "802-11-wireless"}
          <button class="small" onclick={() => openEditor(p.uuid)}>Edit</button>
        {/if}
        <button class="small" onclick={() => toggle(p.uuid, !p.active)}>{p.active ? "Disconnect" : "Connect"}</button>
      </div>
    {/each}
  </div>
{/if}

{#if edit}
  {@const e = edit}
  <Modal title="IP settings — {e.name}" onclose={() => !saving && (edit = null)} width={640}>
    <div class="editor">
      <section>
        <h4>IPv4</h4>
        <div class="methods">
          {#each [["auto", "Automatic (DHCP)"], ["manual", "Manual (static)"], ["link-local", "Link-local only"], ["disabled", "Disabled"]] as [v, l] (v)}
            <label class="opt"><input type="radio" bind:group={e.ipv4.method} value={v} /> {l}</label>
          {/each}
        </div>
        {#if e.ipv4.method === "manual"}
          <div class="addrs">
            {#each addr4 as a, i (i)}
              <div class="arow">
                <input class="mono ipin" class:bad={a.ip && !isV4(a.ip)} placeholder="192.168.1.50" bind:value={a.ip} />
                <span class="muted">/</span>
                <select bind:value={a.prefix}>{#each Array.from({ length: 32 }, (_, k) => 32 - k) as p (p)}<option value={p}>{p} ({mask(p)})</option>{/each}</select>
                <button class="ghost small" onclick={() => (addr4 = addr4.filter((_, k) => k !== i))} aria-label="Remove address"><Icon name="close" size={13} /></button>
              </div>
            {/each}
            <button class="ghost small add" onclick={() => (addr4 = [...addr4, { ip: "", prefix: 24 }])}>+ Add address</button>
          </div>
          <label class="fld">Gateway <input class="mono" placeholder="192.168.1.1" bind:value={e.ipv4.gateway} /></label>
        {/if}
        <label class="fld">DNS servers <input class="mono" placeholder="1.1.1.1, 8.8.8.8" bind:value={dns4} /></label>
        {#if e.ipv4.method === "auto"}
          <label class="opt"><input type="checkbox" bind:checked={e.ipv4.ignoreAutoDns} /> Use only these DNS servers (ignore DHCP DNS)</label>
        {/if}
      </section>

      <section>
        <h4>IPv6</h4>
        <div class="methods">
          {#each [["auto", "Automatic"], ["dhcp", "DHCPv6 only"], ["manual", "Manual"], ["link-local", "Link-local"], ["disabled", "Disabled"]] as [v, l] (v)}
            <label class="opt"><input type="radio" bind:group={e.ipv6.method} value={v} /> {l}</label>
          {/each}
        </div>
        {#if e.ipv6.method === "manual"}
          <div class="addrs">
            {#each addr6 as a, i (i)}
              <div class="arow">
                <input class="mono ipin" placeholder="fd00::50" bind:value={a.ip} /><span class="muted">/</span>
                <input class="pfx" type="number" min="1" max="128" bind:value={a.prefix} />
                <button class="ghost small" onclick={() => (addr6 = addr6.filter((_, k) => k !== i))} aria-label="Remove address"><Icon name="close" size={13} /></button>
              </div>
            {/each}
            <button class="ghost small add" onclick={() => (addr6 = [...addr6, { ip: "", prefix: 64 }])}>+ Add address</button>
          </div>
          <label class="fld">Gateway <input class="mono" bind:value={e.ipv6.gateway} /></label>
        {/if}
        <label class="fld">DNS servers <input class="mono" placeholder="2606:4700:4700::1111" bind:value={dns6} /></label>
      </section>

      <section class="row2">
        <label class="opt"><input type="checkbox" bind:checked={mtuAuto} /> Automatic MTU</label>
        {#if !mtuAuto}<input class="pfx" type="number" min="576" max="9216" bind:value={e.mtu} placeholder="1500" />{/if}
        <span class="grow"></span>
        <label class="opt"><input type="checkbox" bind:checked={e.autoconnect} /> Connect automatically</label>
      </section>

      {#if errors.length}<ul class="errs">{#each errors as er (er)}<li>{er}</li>{/each}</ul>{/if}
      <p class="muted small"><Icon name="info" size={12} /> "Save & apply" reconnects this adapter; the network drops for a few seconds. Remote sessions over it may disconnect.</p>
    </div>
    {#snippet footer()}
      <button onclick={() => (edit = null)} disabled={saving}>Cancel</button>
      <button onclick={() => save(false)} disabled={saving || errors.length > 0}>Save</button>
      <button class="primary" onclick={() => (confirmApply = true)} disabled={saving || errors.length > 0}>{saving ? "Applying…" : "Save & apply"}</button>
    {/snippet}
  </Modal>
{/if}

{#if confirmApply && edit}
  <Modal title="Apply new IP settings now?" onclose={() => (confirmApply = false)}>
    "{edit.name}" will reconnect with the new settings. If the address or gateway is wrong, this computer may lose network access until you fix it here.
    {#snippet footer()}
      <button onclick={() => (confirmApply = false)}>Cancel</button>
      <button class="primary" onclick={() => save(true)}>Apply</button>
    {/snippet}
  </Modal>
{/if}

<style>
  button :global(svg) { vertical-align: -2px; }
  .seg { display: inline-flex; background: var(--surface-2); border: 1px solid var(--border); border-radius: 10px; padding: 3px; gap: 2px; }
  .seg button { border: none; background: transparent; padding: 4px 12px; border-radius: 7px; color: var(--text-secondary); font-size: 13px; }
  .seg button.on { background: var(--surface-3); color: var(--text-primary); }
  .small { font-size: 12.5px; padding: 4px 10px; }
  .grow { flex: 1; }
  .cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(440px, 1fr)); gap: 14px; }
  .dev { background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); padding: 16px 18px; display: flex; flex-direction: column; gap: 12px; }
  .dev header { display: flex; gap: 12px; align-items: center; }
  .ic { width: 36px; height: 36px; border-radius: 10px; display: grid; place-items: center; background: var(--surface-2); color: var(--text-muted); flex: none; }
  .ic.up { color: var(--good); background: color-mix(in srgb, var(--good) 12%, transparent); }
  .tt { flex: 1; min-width: 0; }
  .tt h3 { font-size: 15px; }
  .tt .muted { font-size: 12px; margin-top: 2px; }
  .pill { font-size: 11.5px; border: 1px solid var(--border-strong); border-radius: 99px; padding: 2px 9px; white-space: nowrap; color: var(--text-secondary); }
  .pill.good { color: var(--good); border-color: currentColor; }
  .wifi { display: flex; align-items: center; gap: 8px; font-size: 13px; background: var(--surface-2); padding: 8px 10px; border-radius: 8px; }
  .sig { display: inline-flex; align-items: flex-end; gap: 2px; height: 18px; }
  .sig i { width: 4px; background: var(--surface-3); border-radius: 1px; }
  .sig i.on { background: var(--good); }
  dl { display: grid; grid-template-columns: 80px 1fr; gap: 6px 12px; margin: 0; font-size: 12.5px; }
  dt { color: var(--text-muted); }
  dd { margin: 0; white-space: pre-line; word-break: break-all; }
  .dev footer { display: flex; gap: 8px; align-items: center; margin-top: auto; }
  .wbar { display: flex; gap: 10px; align-items: center; margin-bottom: 12px; }
  .radio { display: inline-flex; gap: 6px; align-items: center; font-size: 13px; }
  .radio input, .opt input { accent-color: var(--accent); }
  .chan { display: flex; gap: 24px; flex-wrap: wrap; background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); padding: 12px 16px; margin-bottom: 12px; }
  .bars { display: flex; align-items: flex-end; gap: 6px; height: 86px; margin-top: 6px; }
  .col { display: flex; flex-direction: column; align-items: center; gap: 4px; }
  .bar { width: 18px; background: var(--series-1); border-radius: 4px 4px 0 0; opacity: 0.7; }
  .bar.mine { background: var(--good); opacity: 1; }
  .cn { font-size: 10.5px; color: var(--text-muted); }
  .table { background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); overflow: hidden; }
  .wrow { display: grid; grid-template-columns: 26px minmax(0, 1fr) 46px 110px 100px 110px 100px; gap: 10px; align-items: center; padding: 9px 14px; border-bottom: 1px solid var(--border); font-size: 13px; }
  .wrow.cur { background: color-mix(in srgb, var(--good) 8%, transparent); }
  .meta { min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .sec { font-size: 11.5px; color: var(--text-secondary); }
  .sec.open { color: var(--warning); }
  .prow { display: grid; grid-template-columns: 10px minmax(0, 1fr) auto auto; gap: 10px; align-items: center; padding: 9px 14px; border-bottom: 1px solid var(--border); font-size: 13px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--surface-3); }
  .dot.on { background: var(--good); }
  .pad { padding: 20px; }
  .editor { display: flex; flex-direction: column; gap: 16px; color: var(--text-primary); }
  .editor h4 { margin: 0 0 8px; font-size: 12px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--text-secondary); }
  .methods { display: flex; flex-wrap: wrap; gap: 6px 16px; margin-bottom: 10px; }
  .opt { display: inline-flex; gap: 6px; align-items: center; font-size: 13px; }
  .addrs { display: flex; flex-direction: column; gap: 6px; margin-bottom: 8px; }
  .arow { display: flex; gap: 6px; align-items: center; }
  .ipin { width: 220px; }
  .ipin.bad { border-color: var(--critical); }
  .pfx { width: 90px; }
  .add { align-self: flex-start; color: var(--accent); }
  .fld { display: flex; align-items: center; gap: 10px; font-size: 13px; margin-bottom: 8px; color: var(--text-secondary); }
  .fld input { flex: 1; }
  .row2 { display: flex; gap: 12px; align-items: center; }
  .errs { margin: 0; padding: 8px 12px 8px 28px; background: color-mix(in srgb, var(--critical) 12%, transparent); border-radius: 8px; color: var(--critical); font-size: 12.5px; }
</style>
