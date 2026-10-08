<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/pages/Connections.svelte
  Author : Praneeth
  License: MIT
  Purpose: System-wide network sockets (listening ports and connections) with
           owning process, filters and auto refresh - like `ss -tunap`.
-->
<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import Icon from "../lib/components/Icon.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import { api } from "../lib/api";
  import { app } from "../lib/stores.svelte";
  import type { Socket } from "../lib/types";

  const PORTS: Record<number, string> = {
    20: "ftp-data", 21: "ftp", 22: "ssh", 25: "smtp", 53: "dns", 67: "dhcp", 68: "dhcp", 80: "http", 110: "pop3", 123: "ntp",
    137: "netbios", 138: "netbios", 139: "smb", 143: "imap", 443: "https", 445: "smb", 465: "smtps", 587: "smtp", 631: "printing",
    993: "imaps", 995: "pop3s", 1194: "openvpn", 1900: "ssdp", 3000: "dev-server", 3306: "mysql", 3389: "rdp", 5353: "mdns",
    5432: "postgres", 5900: "vnc", 6379: "redis", 7070: "anydesk", 8080: "http-alt", 8443: "https-alt", 9000: "dev-server",
    11434: "ollama", 27017: "mongodb", 51820: "wireguard",
  };
  type Filter = "all" | "listening" | "established" | "udp";

  let sockets = $state<Socket[]>([]);
  let filter = $state<Filter>("all");
  let query = $state("");
  let paused = $state(false);
  let loaded = $state(false);
  let timer: ReturnType<typeof setInterval>;

  async function load() {
    if (paused) return;
    try {
      sockets = await api.listConnections();
      loaded = true;
    } catch (e) {
      app.notify(String(e), "error");
    }
  }
  onMount(() => {
    load();
    timer = setInterval(load, 3000);
  });
  onDestroy(() => clearInterval(timer));

  const isListen = (s: Socket) => s.state === "LISTEN" || (s.protocol.startsWith("udp") && s.state === "UNCONN");
  let counts = $derived({
    listening: sockets.filter(isListen).length,
    established: sockets.filter((s) => s.state === "ESTABLISHED").length,
    remoteHosts: new Set(sockets.filter((s) => s.scope === "remote" && s.state === "ESTABLISHED").map((s) => s.remoteAddress)).size,
    unknown: sockets.filter((s) => s.pid == null).length,
  });
  let rows = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return sockets
      .filter((s) => {
        if (filter === "listening" && !isListen(s)) return false;
        if (filter === "established" && s.state !== "ESTABLISHED") return false;
        if (filter === "udp" && !s.protocol.startsWith("udp")) return false;
        if (!q) return true;
        return [s.process ?? "", s.localAddress, s.remoteAddress, String(s.localPort), String(s.remotePort), s.user, PORTS[s.localPort] ?? "", PORTS[s.remotePort] ?? ""]
          .some((x) => x.toLowerCase().includes(q));
      })
      .sort((a, b) => Number(isListen(b)) - Number(isListen(a)) || a.localPort - b.localPort);
  });

  const ep = (addr: string, port: number) => (addr.includes(":") ? `[${addr}]:${port}` : `${addr}:${port}`);
  const exposed = (s: Socket) => isListen(s) && (s.localAddress === "0.0.0.0" || s.localAddress === "::");
</script>

<PageHeader title="Connections" subtitle="Open ports and network connections, refreshed every 3 s">
  {#snippet actions()}
    <div class="search">
      <Icon name="search" size={15} />
      <input placeholder="Process, address, port…" bind:value={query} aria-label="Search connections" />
    </div>
    <button onclick={() => (paused = !paused)}><Icon name={paused ? "play" : "stop"} size={14} /> {paused ? "Resume" : "Pause"}</button>
  {/snippet}
</PageHeader>

<div class="tiles">
  <button class="tile" class:on={filter === "all"} onclick={() => (filter = "all")}><span class="big num">{sockets.length}</span><span class="muted">All sockets</span></button>
  <button class="tile" class:on={filter === "listening"} onclick={() => (filter = "listening")}><span class="big num" style:color="var(--accent)">{counts.listening}</span><span class="muted">Listening</span></button>
  <button class="tile" class:on={filter === "established"} onclick={() => (filter = "established")}><span class="big num" style:color="var(--good)">{counts.established}</span><span class="muted">Established · {counts.remoteHosts} remote hosts</span></button>
  <button class="tile" class:on={filter === "udp"} onclick={() => (filter = "udp")}><span class="big num">{sockets.filter((s) => s.protocol.startsWith("udp")).length}</span><span class="muted">UDP</span></button>
</div>

{#if counts.unknown}
  <p class="muted note"><Icon name="info" size={13} /> {counts.unknown} socket(s) belong to system services; their process names need administrator access and are shown by user only.</p>
{/if}

<div class="tablewrap">
  <table>
    <thead>
      <tr><th>Proto</th><th>Local address</th><th>Remote address</th><th>State</th><th>Process</th><th>User</th></tr>
    </thead>
    <tbody>
      {#each rows as s, i (s.protocol + s.localAddress + s.localPort + s.remoteAddress + s.remotePort + i)}
        <tr>
          <td class="muted">{s.protocol}</td>
          <td class="mono">
            {ep(s.localAddress, s.localPort)}
            {#if PORTS[s.localPort] && isListen(s)}<span class="svc">{PORTS[s.localPort]}</span>{/if}
            {#if exposed(s)}<span class="exp" title="Reachable from other computers on the network">network</span>{/if}
          </td>
          <td class="mono">
            {#if isListen(s)}<span class="muted">—</span>{:else}{ep(s.remoteAddress, s.remotePort)}{#if PORTS[s.remotePort]}<span class="svc">{PORTS[s.remotePort]}</span>{/if}{/if}
          </td>
          <td><span class="st {s.state.toLowerCase()}">{s.state === "UNCONN" ? "open" : s.state.toLowerCase().replace("_", " ")}</span></td>
          <td>{#if s.process}<b>{s.process}</b> <span class="muted num">{s.pid}</span>{:else}<span class="muted">system</span>{/if}</td>
          <td class="muted">{s.user}</td>
        </tr>
      {/each}
    </tbody>
  </table>
  {#if loaded && !rows.length}<div class="muted pad">No sockets match.</div>{/if}
</div>

<style>
  button :global(svg) { vertical-align: -2px; }
  .search { display: flex; align-items: center; gap: 6px; background: var(--surface-2); border: 1px solid var(--border-strong); border-radius: 8px; padding: 0 10px; color: var(--text-muted); }
  .search input { border: none; background: transparent; width: 220px; padding: 7px 0; }
  .tiles { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 10px; margin-bottom: 12px; }
  .tile { display: flex; flex-direction: column; align-items: flex-start; gap: 2px; padding: 10px 14px; background: var(--surface-1); border: 1px solid var(--border); border-radius: 12px; text-align: left; }
  .tile.on { border-color: var(--accent); }
  .tile .big { font-size: 22px; font-weight: 650; }
  .tile .muted { font-size: 12px; }
  .note { font-size: 12.5px; display: flex; gap: 6px; align-items: center; margin: 0 0 10px; }
  .tablewrap { background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); overflow: auto; max-height: calc(100vh - 290px); }
  table { width: 100%; border-collapse: collapse; font-size: 13px; }
  th { position: sticky; top: 0; background: var(--surface-2); text-align: left; font-weight: 500; font-size: 12px; color: var(--text-secondary); padding: 9px 12px; border-bottom: 1px solid var(--border); }
  td { padding: 7px 12px; border-bottom: 1px solid var(--border); white-space: nowrap; }
  tbody tr:hover { background: var(--surface-2); }
  .mono { font-size: 12px; }
  .svc { font-family: var(--font-sans); font-size: 10.5px; margin-left: 6px; color: var(--text-secondary); background: var(--surface-3); padding: 1px 6px; border-radius: 99px; }
  .exp { font-family: var(--font-sans); font-size: 10.5px; margin-left: 6px; color: var(--warning); border: 1px solid currentColor; padding: 0 6px; border-radius: 99px; }
  .st { font-size: 11.5px; text-transform: capitalize; color: var(--text-secondary); }
  .st.listen, .st.unconn { color: var(--accent); }
  .st.established { color: var(--good); }
  .st.time_wait, .st.close_wait { color: var(--text-muted); }
  .pad { padding: 20px; }
</style>
