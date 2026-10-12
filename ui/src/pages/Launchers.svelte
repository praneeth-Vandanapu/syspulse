<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/pages/Launchers.svelte
  Author : Praneeth
  License: MIT
  Purpose: App Launchers - make an application folder (C++ build, Python app,
           AppImage, script, jar) appear as a normal desktop application for
           chosen user accounts: start menu, trusted desktop icon, dock.
           Left: launchers created so far. Right/main: the editor with live
           checks (permissions per user, missing libraries, desktop file).
-->
<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "../lib/components/Icon.svelte";
  import Modal from "../lib/components/Modal.svelte";
  import { app } from "../lib/stores.svelte";
  import { pickFolder, pickFile, isTauri } from "../lib/api";
  import { bytes as fmtBytes } from "../lib/format";
  import {
    ln, CATEGORIES, KIND_LABEL,
    type LauncherSpec, type ScanResult, type UserAccount, type CheckReport, type LauncherRecord, type UserResult, type Candidate,
  } from "../lib/launchers";

  let users = $state<UserAccount[]>([]);
  let me = $state("");
  let aclAvailable = $state(true);
  let list = $state<LauncherRecord[]>([]);

  // editor
  let editing = $state(false);
  let editingExisting = $state(false);
  let scan = $state<ScanResult | null>(null);
  let scanning = $state(false);
  let spec = $state<LauncherSpec | null>(null);
  let targets = $state<string[]>([]);
  let report = $state<CheckReport | null>(null);
  let checking = $state(false);
  let creating = $state(false);
  let showAdvanced = $state(false);
  let showPreview = $state(false);
  let customIconPreview = $state<string | null>(null);
  let results = $state<UserResult[] | null>(null);
  let testing = $state(false);
  let testOut = $state<{ running: boolean; exitCode: number | null; output: string } | null>(null);
  let removeDlg = $state<{ rec: LauncherRecord; users: string[]; revoke: boolean } | null>(null);

  async function refresh() {
    try {
      const u = await ln.users();
      users = u.users;
      me = u.me;
      aclAvailable = u.aclAvailable;
      list = await ln.list();
    } catch (e) {
      app.notify(String(e), "error");
    }
  }
  onMount(refresh);

  function newLauncher() {
    editing = true;
    editingExisting = false;
    scan = null;
    spec = null;
    report = null;
    results = null;
    testOut = null;
    customIconPreview = null;
    targets = users.filter((u) => u.name !== me).slice(0, 1).map((u) => u.name);
    if (targets.length === 0 && me) targets = [me];
    chooseFolder();
  }

  async function chooseFolder() {
    // Browser preview (no Tauri): use the mock backend's demo folder.
    const f = isTauri ? await pickFolder() : "/opt/apps/SignalViewer";
    if (!f) {
      if (!scan) editing = false;
      return;
    }
    await loadFolder(f);
  }

  async function loadFolder(folder: string, keep?: LauncherSpec) {
    scanning = true;
    try {
      const r = await ln.scan(folder);
      scan = r.scan;
      if (keep) {
        spec = keep;
      } else {
        const best = r.scan.candidates[0];
        spec = {
          id: r.suggestedId,
          name: r.scan.name,
          comment: "",
          folder: r.scan.folder,
          program: best?.path ?? "",
          kind: best?.kind ?? "native",
          interpreter: null,
          args: "",
          workdir: "",
          env: r.suggestedEnv,
          terminal: false,
          category: "Utility",
          wmClass: "",
          icon: r.scan.icons[0]?.path ?? "",
          menu: true,
          desktop: true,
          dock: true,
          grantAccess: true,
          grantWrite: false,
        };
        if (best) applyCandidate(best);
      }
    } catch (e) {
      app.notify(String(e), "error");
    } finally {
      scanning = false;
    }
  }

  function applyCandidate(c: Candidate) {
    if (!spec || !scan) return;
    spec.program = c.path;
    spec.kind = c.kind;
    if (c.kind === "python") spec.interpreter = scan.venvPython ?? "/usr/bin/python3";
    else if (c.kind === "jar") spec.interpreter = "/usr/bin/java";
    else if (c.kind === "script" && !c.executable) spec.interpreter = "/bin/bash";
    else spec.interpreter = null;
    if (!spec.wmClass && c.kind === "native") spec.wmClass = "";
  }

  async function otherProgram() {
    if (!spec) return;
    const f = await pickFile();
    if (!f) return;
    if (!f.startsWith(spec.folder + "/")) {
      app.notify("The program must be inside the application folder.", "error");
      return;
    }
    const lower = f.toLowerCase();
    const kind = lower.endsWith(".py") ? "python" : lower.endsWith(".jar") ? "jar" : lower.endsWith(".appimage") ? "appImage" : lower.endsWith(".sh") ? "script" : "native";
    applyCandidate({ path: f, rel: f.slice(spec.folder.length + 1), kind, score: 0, reason: "Chosen by you", executable: true, size: 0 });
  }

  async function otherIcon() {
    if (!spec) return;
    const f = await pickFile();
    if (!f) return;
    spec.icon = f;
    customIconPreview = await ln.iconPreview(f).catch(() => null);
  }

  function edit(rec: LauncherRecord) {
    editing = true;
    editingExisting = true;
    results = null;
    testOut = null;
    report = null;
    targets = [...rec.users];
    customIconPreview = rec.iconPreview;
    loadFolder(rec.spec.folder, structuredClone($state.snapshot(rec.spec)) as LauncherSpec);
  }

  // live checks
  let checkTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const snap = spec ? JSON.stringify(spec) : "";
    const t = targets.join(",");
    if (!snap) return;
    clearTimeout(checkTimer);
    checkTimer = setTimeout(async () => {
      checking = true;
      try {
        report = await ln.check(JSON.parse(snap), t ? t.split(",") : []);
      } catch (e) {
        report = null;
      } finally {
        checking = false;
      }
    }, 350);
  });

  let errors = $derived(report?.issues.filter((i) => i.level === "error") ?? []);
  let needsPassword = $derived(targets.some((t) => t !== me));
  let selectedIconPreview = $derived(
    spec ? (scan?.icons.find((i) => i.path === spec!.icon)?.preview ?? customIconPreview) : null,
  );

  async function create() {
    if (!spec) return;
    creating = true;
    try {
      results = await ln.create($state.snapshot(spec) as LauncherSpec, [...targets]);
      const ok = results.filter((r) => r.ok).length;
      app.notify(`Launcher "${spec.name}" ${editingExisting ? "updated" : "created"} for ${ok} of ${results.length} user(s)`, ok === results.length ? "ok" : "error");
      list = await ln.list();
    } catch (e) {
      const msg = String(e);
      app.notify(msg.includes("cancelled") ? "Cancelled — no changes were made." : msg, "error", 7000);
    } finally {
      creating = false;
    }
  }

  async function testRun() {
    if (!spec) return;
    testing = true;
    try {
      testOut = await ln.testRun($state.snapshot(spec) as LauncherSpec);
    } catch (e) {
      testOut = { running: false, exitCode: null, output: String(e) };
    } finally {
      testing = false;
    }
  }

  async function doRemove() {
    if (!removeDlg) return;
    const d = removeDlg;
    removeDlg = null;
    try {
      const r = await ln.remove(d.rec.spec.id, d.users, d.revoke);
      const ok = r.filter((x) => x.ok).length;
      app.notify(`Removed "${d.rec.spec.name}" for ${ok} user(s)`, ok === r.length ? "ok" : "error");
      list = await ln.list();
    } catch (e) {
      const msg = String(e);
      app.notify(msg.includes("cancelled") ? "Cancelled — nothing was removed." : msg, "error", 7000);
    }
  }

  function toggleTarget(name: string) {
    targets = targets.includes(name) ? targets.filter((t) => t !== name) : [...targets, name];
  }
  function userLabel(n: string) {
    const u = users.find((x) => x.name === n);
    return u?.fullName ? `${u.fullName} (${n})` : n;
  }
</script>

<div class="page">
  <div class="head">
    <div>
      <h1>App Launchers</h1>
      <span class="muted">Make an application folder appear as a normal app — start menu, desktop icon and dock — for any user on this PC</span>
    </div>
    <span class="grow"></span>
    {#if !editing}<button class="primary" onclick={newLauncher}><Icon name="play" size={15} /> New launcher</button>{/if}
  </div>

  {#if !editing}
    {#if list.length === 0}
      <div class="empty">
        <Icon name="apps" size={40} />
        <h3>No launchers yet</h3>
        <p class="muted">Choose the folder of a C++ program, Python app, AppImage, script or Java application. SysPulse finds the program and its icon, fixes permissions and libraries, and installs the launcher for the users you pick — without them doing anything.</p>
        <button class="primary" onclick={newLauncher}>Create the first launcher</button>
      </div>
    {:else}
      <div class="cards">
        {#each list as rec (rec.spec.id)}
          <div class="lcard">
            <div class="licon">{#if rec.iconPreview}<img src={rec.iconPreview} alt="" />{:else}<Icon name="apps" size={28} />{/if}</div>
            <div class="linfo">
              <b>{rec.spec.name}</b>
              <span class="muted mono small ell" title={rec.spec.program}>{rec.spec.program}</span>
              <div class="chips">
                {#each rec.users as u (u)}
                  {@const r = rec.results.find((x) => x.user === u)}
                  <span class="uchip" class:warn={!!r && (r.warnings.length > 0 || !r.ok)} title={r ? [...r.done, ...r.warnings].join("\n") : ""}>{u}</span>
                {/each}
                {#if rec.spec.menu}<span class="tag">Menu</span>{/if}
                {#if rec.spec.desktop}<span class="tag">Desktop</span>{/if}
                {#if rec.spec.dock}<span class="tag">Dock</span>{/if}
              </div>
            </div>
            <div class="lacts">
              <button class="small" onclick={() => edit(rec)}>Edit / add users</button>
              <button class="small" onclick={() => (removeDlg = { rec, users: [...rec.users], revoke: rec.results.some((r) => r.aclFolder) })}>Remove…</button>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  {:else}
    <div class="editor">
      <div class="form">
        <!-- 1. folder -->
        <section>
          <h4>1 · Application folder</h4>
          {#if scanning}
            <p class="muted">Scanning the folder…</p>
          {:else if scan}
            <div class="row">
              <span class="mono path">{scan.folder}</span>
              <span class="muted small">{scan.files.toLocaleString()} files · {fmtBytes(scan.bytes)}</span>
              <span class="grow"></span>
              {#if !editingExisting}<button class="small" onclick={chooseFolder}>Change…</button>{/if}
            </div>
            <p class="muted small">The folder stays where it is; the launcher runs the program from there with all its files.</p>
          {:else}
            <button onclick={chooseFolder}>Choose folder…</button>
          {/if}
        </section>

        {#if spec && scan}
          <!-- 2. program -->
          <section>
            <h4>2 · Program to start</h4>
            {#if scan.candidates.length === 0}
              <p class="warn">No program was recognised in this folder. Choose it yourself.</p>
            {/if}
            <div class="cands">
              {#each scan.candidates.slice(0, 6) as c (c.path)}
                <label class="cand" class:on={spec.program === c.path}>
                  <input type="radio" name="prog" checked={spec.program === c.path} onchange={() => applyCandidate(c)} />
                  <span class="mono">{c.rel}</span>
                  <span class="kind">{KIND_LABEL[c.kind]}</span>
                  <span class="muted small">{c.reason}{c.size ? ` · ${fmtBytes(c.size)}` : ""}</span>
                </label>
              {/each}
              {#if !scan.candidates.some((c) => c.path === spec!.program) && spec.program}
                <label class="cand on"><input type="radio" checked /> <span class="mono">{spec.program.slice(spec.folder.length + 1)}</span> <span class="kind">{KIND_LABEL[spec.kind]}</span></label>
              {/if}
            </div>
            <button class="small ghost" onclick={otherProgram}>Choose another file…</button>
            {#if spec.kind === "python" || spec.kind === "jar" || spec.interpreter}
              <label class="field">Run with
                <input class="mono" bind:value={spec.interpreter} placeholder={spec.kind === "jar" ? "/usr/bin/java" : "/usr/bin/python3"} />
              </label>
              {#if spec.kind === "python" && scan.venvPython}<p class="muted small">Using the virtual environment found in the folder.</p>{/if}
            {/if}
          </section>

          <!-- 3. look -->
          <section>
            <h4>3 · Name and icon</h4>
            <div class="grid2">
              <label class="field">Name <input bind:value={spec.name} /></label>
              <label class="field">Menu category
                <select bind:value={spec.category}>
                  {#each CATEGORIES as [v, l] (v)}<option value={v}>{l}</option>{/each}
                </select>
              </label>
            </div>
            <label class="field">Description <input bind:value={spec.comment} placeholder="Shown as a tooltip in the menu" /></label>
            <div class="icons">
              {#each scan.icons.slice(0, 10) as i (i.path)}
                <button class="ic" class:on={spec.icon === i.path} title="{i.rel}{i.width ? ` (${i.width}×${i.height})` : ''}" onclick={() => (spec!.icon = i.path)}>
                  {#if i.preview}<img src={i.preview} alt="" />{:else}<span class="small muted">{i.rel.split(".").pop()}</span>{/if}
                </button>
              {/each}
              {#if spec.icon && !scan.icons.some((i) => i.path === spec!.icon)}
                <button class="ic on" title={spec.icon}>{#if customIconPreview}<img src={customIconPreview} alt="" />{:else}<Icon name="files" size={20} />{/if}</button>
              {/if}
              <button class="ic add" onclick={otherIcon} title="Choose an icon file (PNG, SVG, ICO, JPG, BMP, XPM)">+</button>
            </div>
            {#if scan.icons.length === 0 && !spec.icon}<p class="muted small">No icon found in the folder — choose one, or a generic icon is used.</p>{/if}
          </section>

          <!-- 4. advanced -->
          <section>
            <button class="disclose" onclick={() => (showAdvanced = !showAdvanced)}>{showAdvanced ? "▾" : "▸"} Run settings <span class="muted small">(arguments, working folder, libraries, terminal)</span></button>
            {#if showAdvanced}
              <label class="field">Arguments <input class="mono" bind:value={spec.args} placeholder={'e.g. --config "my settings.ini"'} /></label>
              <label class="field">Working folder <input class="mono" bind:value={spec.workdir} placeholder="Default: the program's folder" /></label>
              <div class="envs">
                <span class="lbl">Environment</span>
                {#each spec.env as e, i (i)}
                  <div class="env">
                    <input class="mono n" bind:value={e.name} />
                    <span>=</span>
                    <input class="mono v" bind:value={e.value} />
                    <button class="x" onclick={() => spec!.env.splice(i, 1)} title="Remove">✕</button>
                  </div>
                {/each}
                <button class="small ghost" onclick={() => spec!.env.push({ name: "", value: "" })}>+ Add variable</button>
                {#if scan.libraryDirs.length || scan.qtPluginDir}
                  <p class="muted small">Detected bundled libraries{scan.qtPluginDir ? " and Qt plugins" : ""} — their folders are already set so the program finds them.</p>
                {/if}
              </div>
              <div class="grid2">
                <label class="field">Window class <input class="mono" bind:value={spec.wmClass} placeholder="optional — groups windows in the dock" /></label>
                <label class="field">Launcher id <input class="mono" bind:value={spec.id} disabled={editingExisting} /></label>
              </div>
              <label class="chk"><input type="checkbox" bind:checked={spec.terminal} /> Run in a terminal window (console programs)</label>
            {/if}
          </section>

          <!-- 5. users -->
          <section>
            <h4>4 · Who gets it, and where</h4>
            <div class="users">
              {#each users as u (u.name)}
                <label class="user" class:on={targets.includes(u.name)}>
                  <input type="checkbox" checked={targets.includes(u.name)} onchange={() => toggleTarget(u.name)} />
                  <span><b>{u.fullName || u.name}</b> <span class="muted small">{u.name}{u.name === me ? " · you" : ""}{u.isAdmin ? " · admin" : ""}</span></span>
                  {#if u.loggedIn}<span class="tag good">logged in</span>{/if}
                </label>
              {/each}
            </div>
            <div class="places">
              <label class="chk"><input type="checkbox" bind:checked={spec.menu} /> Start menu</label>
              <label class="chk"><input type="checkbox" bind:checked={spec.desktop} /> Desktop icon</label>
              <label class="chk"><input type="checkbox" bind:checked={spec.dock} /> Pin to the dock</label>
            </div>
            <div class="places">
              <label class="chk"><input type="checkbox" bind:checked={spec.grantAccess} disabled={!aclAvailable} /> Give these users access to the folder if they need it</label>
              <label class="chk"><input type="checkbox" bind:checked={spec.grantWrite} disabled={!spec.grantAccess} /> …and let them write in it (apps that save logs or settings next to the program)</label>
            </div>
            {#if !aclAvailable}<p class="warn small">Folder access cannot be granted: install the acl package (sudo apt install acl).</p>{/if}
          </section>
        {/if}
      </div>

      <!-- checks / actions -->
      <aside class="side">
        {#if spec}
          <div class="previewcard">
            <div class="pv">{#if selectedIconPreview}<img src={selectedIconPreview} alt="" />{:else}<Icon name="apps" size={34} />{/if}</div>
            <b>{spec.name || "Untitled"}</b>
            <span class="muted small">{CATEGORIES.find((c) => c[0] === spec!.category)?.[1]}</span>
          </div>
          <div class="checks">
            <h4>Checks {#if checking}<span class="muted small">…</span>{/if}</h4>
            {#if report}
              {#each report.issues as i, k (k)}
                <div class="issue {i.level}">
                  <span class="dot"></span>
                  <span>{i.text}</span>
                </div>
              {/each}
              {#if report.issues.length === 0}<div class="issue ok"><span class="dot"></span><span>Ready — no problems found.</span></div>{/if}
              <button class="disclose small" onclick={() => (showPreview = !showPreview)}>{showPreview ? "▾" : "▸"} Desktop file preview</button>
              {#if showPreview}<pre class="mono">{report.preview}</pre>{/if}
            {/if}
          </div>
          <div class="actions">
            <button onclick={testRun} disabled={testing || !spec.program} title="Start the program now, as you, to see that it runs">{testing ? "Starting…" : "Test run"}</button>
            <button class="primary" onclick={create} disabled={creating || errors.length > 0 || targets.length === 0}>
              {creating ? "Working…" : editingExisting ? "Update launcher" : "Create launcher"}
            </button>
            <button class="ghost" onclick={() => { editing = false; refresh(); }}>Close</button>
          </div>
          {#if targets.length === 0}<p class="muted small">Choose at least one user.</p>{/if}
          {#if needsPassword && targets.length}<p class="muted small">Installing for other accounts asks for your password once.</p>{/if}
          {#if testOut}
            <div class="testout">
              <b>{testOut.running ? "The program started and is running." : `The program exited (code ${testOut.exitCode ?? "?"}).`}</b>
              {#if testOut.output.trim()}<pre class="mono">{testOut.output}</pre>{/if}
            </div>
          {/if}
          {#if results}
            <div class="results">
              {#each results as r (r.user)}
                <div class="res" class:bad={!r.ok}>
                  <b>{userLabel(r.user)}: {r.ok ? "done" : "failed"}</b>
                  <ul>
                    {#each r.done as d, k (k)}<li>✔ {d}</li>{/each}
                    {#each r.warnings as w, k (k)}<li class="warn">⚠ {w}</li>{/each}
                    {#if r.error}<li class="err">✖ {r.error}</li>{/if}
                  </ul>
                </div>
              {/each}
            </div>
          {/if}
        {/if}
      </aside>
    </div>
  {/if}
</div>

{#if removeDlg}
  {@const d = removeDlg}
  <Modal title="Remove “{d.rec.spec.name}”" onclose={() => (removeDlg = null)}>
    <p>Remove the start-menu entry, desktop icon, dock pin and icon for:</p>
    {#each d.rec.users as u (u)}
      <label class="chk"><input type="checkbox" checked={d.users.includes(u)} onchange={() => (d.users = d.users.includes(u) ? d.users.filter((x) => x !== u) : [...d.users, u])} /> {userLabel(u)}</label>
    {/each}
    {#if d.rec.results.some((r) => r.aclFolder)}
      <label class="chk"><input type="checkbox" bind:checked={d.revoke} /> Also remove their access to {d.rec.spec.folder}</label>
    {/if}
    <p class="muted small">The application folder itself is never deleted.</p>
    {#snippet footer()}
      <button onclick={() => (removeDlg = null)}>Cancel</button>
      <button class="danger" disabled={d.users.length === 0} onclick={doRemove}>Remove</button>
    {/snippet}
  </Modal>
{/if}

<style>
  button :global(svg) { vertical-align: -2px; }
  .page { display: flex; flex-direction: column; gap: 14px; }
  .head { display: flex; align-items: center; gap: 10px; }
  .head .muted { font-size: 12.5px; }
  .grow { flex: 1; }
  .small { font-size: 12px; }
  .muted.small { font-size: 12px; }
  .warn { color: var(--warning); }
  .err { color: var(--critical); }
  .empty { display: flex; flex-direction: column; align-items: center; text-align: center; gap: 8px; padding: 50px 20px; background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); color: var(--text-secondary); }
  .empty p { max-width: 620px; }
  .cards { display: flex; flex-direction: column; gap: 8px; }
  .lcard { display: flex; gap: 14px; align-items: center; background: var(--surface-1); border: 1px solid var(--border); border-radius: 12px; padding: 12px 14px; }
  .licon { width: 48px; height: 48px; display: grid; place-items: center; background: var(--surface-2); border-radius: 10px; flex: none; color: var(--text-muted); }
  .licon img { width: 40px; height: 40px; object-fit: contain; }
  .linfo { display: flex; flex-direction: column; gap: 3px; min-width: 0; flex: 1; }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chips { display: flex; gap: 5px; flex-wrap: wrap; margin-top: 3px; }
  .uchip { font-size: 11.5px; padding: 1px 8px; border-radius: 999px; background: color-mix(in srgb, var(--accent) 16%, transparent); border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent); }
  .uchip.warn { background: color-mix(in srgb, var(--warning) 14%, transparent); border-color: color-mix(in srgb, var(--warning) 45%, transparent); }
  .tag { font-size: 11px; padding: 1px 7px; border-radius: 6px; background: var(--surface-3); color: var(--text-secondary); }
  .tag.good { background: color-mix(in srgb, var(--good) 18%, transparent); color: var(--good); }
  .lacts { display: flex; gap: 6px; }
  .editor { display: grid; grid-template-columns: minmax(0, 1fr) 330px; gap: 14px; align-items: start; }
  .form { display: flex; flex-direction: column; gap: 10px; }
  section { background: var(--surface-1); border: 1px solid var(--border); border-radius: 12px; padding: 12px 14px; display: flex; flex-direction: column; gap: 8px; }
  h4 { margin: 0; font-size: 12px; text-transform: uppercase; letter-spacing: 0.05em; color: var(--text-secondary); }
  .row { display: flex; gap: 10px; align-items: center; flex-wrap: wrap; }
  .path { font-size: 12.5px; word-break: break-all; }
  .cands { display: flex; flex-direction: column; gap: 4px; }
  .cand { display: flex; gap: 8px; align-items: center; padding: 6px 8px; border: 1px solid var(--border); border-radius: 8px; cursor: pointer; font-size: 13px; }
  .cand.on { border-color: var(--accent); background: color-mix(in srgb, var(--accent) 10%, transparent); }
  .cand input { accent-color: var(--accent); }
  .kind { font-size: 11px; padding: 1px 7px; border-radius: 6px; background: var(--surface-3); }
  .field { display: flex; flex-direction: column; gap: 4px; font-size: 12.5px; color: var(--text-secondary); flex: 1; }
  .field input, .field select { color: var(--text-primary); font-size: 13px; }
  .grid2 { display: flex; gap: 10px; flex-wrap: wrap; }
  .icons { display: flex; gap: 6px; flex-wrap: wrap; }
  .ic { width: 52px; height: 52px; padding: 4px; display: grid; place-items: center; border-radius: 10px; }
  .ic img { max-width: 40px; max-height: 40px; object-fit: contain; }
  .ic.on { border-color: var(--accent); border-width: 2px; background: color-mix(in srgb, var(--accent) 12%, var(--surface-2)); }
  .ic.add { font-size: 22px; color: var(--text-muted); }
  .disclose { all: unset; cursor: pointer; font-size: 13px; color: var(--text-primary); }
  .envs { display: flex; flex-direction: column; gap: 5px; }
  .lbl { font-size: 12.5px; color: var(--text-secondary); }
  .env { display: flex; gap: 6px; align-items: center; }
  .env .n { width: 190px; font-size: 12.5px; }
  .env .v { flex: 1; font-size: 12.5px; }
  .x { border: none; background: transparent; color: var(--text-muted); padding: 2px 6px; }
  .chk { display: inline-flex; gap: 6px; align-items: center; font-size: 13px; }
  .chk input { accent-color: var(--accent); }
  .users { display: flex; flex-direction: column; gap: 4px; }
  .user { display: flex; gap: 8px; align-items: center; padding: 6px 8px; border: 1px solid var(--border); border-radius: 8px; cursor: pointer; font-size: 13px; }
  .user.on { border-color: var(--accent); background: color-mix(in srgb, var(--accent) 10%, transparent); }
  .user input { accent-color: var(--accent); }
  .places { display: flex; gap: 16px; flex-wrap: wrap; }
  .side { display: flex; flex-direction: column; gap: 10px; position: sticky; top: 0; }
  .previewcard { display: flex; flex-direction: column; align-items: center; gap: 4px; padding: 14px; background: var(--surface-1); border: 1px solid var(--border); border-radius: 12px; text-align: center; }
  .pv { width: 72px; height: 72px; display: grid; place-items: center; color: var(--text-muted); }
  .pv img { max-width: 64px; max-height: 64px; object-fit: contain; }
  .checks { background: var(--surface-1); border: 1px solid var(--border); border-radius: 12px; padding: 12px; display: flex; flex-direction: column; gap: 6px; }
  .issue { display: flex; gap: 8px; font-size: 12.5px; line-height: 1.4; }
  .issue .dot { width: 8px; height: 8px; border-radius: 50%; margin-top: 5px; flex: none; }
  .issue.error .dot { background: var(--critical); }
  .issue.warning .dot { background: var(--warning); }
  .issue.fix .dot { background: var(--accent); }
  .issue.ok .dot { background: var(--good); }
  pre { font-size: 11px; white-space: pre-wrap; word-break: break-all; background: var(--surface-2); border-radius: 8px; padding: 8px; margin: 0; max-height: 240px; overflow: auto; }
  .actions { display: flex; gap: 6px; flex-wrap: wrap; }
  .testout, .results { background: var(--surface-1); border: 1px solid var(--border); border-radius: 12px; padding: 10px 12px; font-size: 12.5px; display: flex; flex-direction: column; gap: 6px; }
  .res ul { margin: 4px 0 0; padding-left: 4px; list-style: none; display: flex; flex-direction: column; gap: 2px; }
  .res.bad b { color: var(--critical); }
</style>
