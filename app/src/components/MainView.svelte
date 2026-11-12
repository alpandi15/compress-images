<script lang="ts">
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { open } from "@tauri-apps/plugin-dialog";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { onDestroy, onMount } from "svelte";
  import { displayPath, humanBytes, percentChange } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import { Job } from "../lib/job.svelte";
  import { settings } from "../lib/settings.svelte";
  import ResultList from "./ResultList.svelte";
  import SettingsPanel from "./SettingsPanel.svelte";

  const IMAGE_EXTENSIONS = ["jpg", "jpeg", "jfif", "png", "webp", "avif", "tif", "tiff", "bmp", "heic", "heif"];

  const job = new Job();
  let inputs = $state<string[]>([]);
  let dragging = $state(false);
  let unlistenDrop: (() => void) | null = null;

  const running = $derived(job.phase === "running");
  const canStart = $derived(inputs.length > 0 && !running && !settings.invalid);
  const outDir = $derived(settings.value?.options.out_dir_name ?? "compressed");

  function addPaths(paths: string[]) {
    const merged = new Set(inputs);
    for (const p of paths) merged.add(p);
    inputs = [...merged];
    if (job.phase === "finished") job.reset();
  }

  async function pickFiles() {
    const picked = await open({
      multiple: true,
      directory: false,
      filters: [{ name: "Images", extensions: IMAGE_EXTENSIONS }],
    });
    if (picked) addPaths(Array.isArray(picked) ? picked : [picked]);
  }

  async function pickFolders() {
    const picked = await open({ multiple: true, directory: true });
    if (picked) addPaths(Array.isArray(picked) ? picked : [picked]);
  }

  async function start() {
    if (!settings.value || !canStart) return;
    await job.start([...inputs], $state.snapshot(settings.value.options));
  }

  function openOutput() {
    if (job.firstOutput) void revealItemInDir(job.firstOutput);
  }

  onMount(async () => {
    unlistenDrop = await getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload;
      if (p.type === "enter" || p.type === "over") dragging = !running;
      else if (p.type === "leave") dragging = false;
      else if (p.type === "drop") {
        dragging = false;
        if (!running) addPaths(p.paths);
      }
    });
  });

  onDestroy(() => {
    unlistenDrop?.();
    job.dispose();
  });
</script>

<div class="layout">
  <main>
    <header>
      <img src="/icon.png" alt="" width="28" height="28" />
      <h1>{t("app.title")}</h1>
    </header>

    <section class="drop" class:dragging aria-label={t("drop.title")}>
      <svg viewBox="0 0 24 24" width="34" height="34" aria-hidden="true">
        <path
          fill="currentColor"
          d="M19 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V5a2 2 0 0 0-2-2Zm0 16H5V5h14v14Zm-5.04-6.71-2.75 3.54-1.96-2.36L6.5 17h11l-3.54-4.71ZM8.5 10A1.5 1.5 0 1 0 8.5 7a1.5 1.5 0 0 0 0 3Z"
        />
      </svg>
      <p class="drop-title">{t("drop.title")}</p>
      <p class="muted drop-hint">{t("drop.hint", { folder: outDir })}</p>
      <div class="drop-actions">
        <button type="button" onclick={pickFiles} disabled={running}>{t("drop.addFiles")}</button>
        <button type="button" onclick={pickFolders} disabled={running}>{t("drop.addFolder")}</button>
      </div>
    </section>

    {#if inputs.length > 0}
      <div class="inputs">
        <div class="inputs-head">
          <span class="muted">{t("inputs.count", { n: inputs.length })}</span>
          <button type="button" class="ghost" onclick={() => (inputs = [])} disabled={running}>
            {t("inputs.clear")}
          </button>
        </div>
        <ul class="chips">
          {#each inputs as path (path)}
            <li title={path}>
              <span>{displayPath(path, [])}</span>
              <button
                type="button"
                class="ghost"
                aria-label={`${t("inputs.remove")} ${path}`}
                disabled={running}
                onclick={() => (inputs = inputs.filter((p) => p !== path))}>×</button
              >
            </li>
          {/each}
        </ul>
      </div>
    {/if}

    <section class="results" aria-live="polite">
      {#each job.inputErrors as err (err.path)}
        <p class="error">{t("errors.input", { path: err.path, error: err.error })}</p>
      {/each}
      {#if job.startError}
        <p class="error">{t("errors.start", { error: job.startError })}</p>
      {/if}
      {#if job.rows.length > 0}
        <ResultList rows={job.rows} roots={inputs} />
      {:else if job.phase === "finished"}
        <p class="muted empty">{t("summary.nothing")}</p>
      {:else}
        <p class="muted empty">{t("empty.results")}</p>
      {/if}
    </section>

    <footer>
      <div class="status">
        {#if running}
          <div class="progress" role="progressbar" aria-valuenow={Math.round(job.progress * 100)} aria-valuemin="0" aria-valuemax="100">
            <div style:width={`${job.progress * 100}%`}></div>
          </div>
          <span class="muted">
            {t("progress.count", { done: job.finishedCount, total: job.encodeTotal })}
            {#if job.bytesSaved > 0}· −{humanBytes(job.bytesSaved)}{/if}
          </span>
        {:else if job.summary}
          {@const s = job.summary}
          <strong>{s.cancelled ? t("summary.cancelled") : t("summary.done")}</strong>
          <span>
            {t("summary.line", {
              compressed: s.compressed + s.kept_original,
              before: humanBytes(s.bytes_before),
              after: humanBytes(s.bytes_after),
              percent: percentChange(s.bytes_before, s.bytes_after),
            })}
            {#if s.failed > 0}· <span class="error-inline">{t("summary.failed", { n: s.failed })}</span>{/if}
            {#if s.skipped > 0}· <span class="muted">{t("summary.skipped", { n: s.skipped })}</span>{/if}
          </span>
        {/if}
      </div>
      <div class="actions">
        {#if job.summary && job.firstOutput && !running}
          <button type="button" onclick={openOutput}>{t("action.openFolder")}</button>
        {/if}
        {#if running}
          <button type="button" onclick={() => job.cancel()}>{t("action.cancel")}</button>
        {:else}
          <button type="button" class="primary" disabled={!canStart} onclick={start}>
            {t("action.compress")}
          </button>
        {/if}
      </div>
    </footer>
  </main>

  <SettingsPanel disabled={running} />
</div>

<style>
  .layout {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 300px;
    height: 100%;
  }

  main {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 18px 20px 0;
    min-height: 0;
  }

  header {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  h1 {
    margin: 0;
    font-size: 17px;
    font-weight: 650;
  }

  .drop {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 4px;
    padding: 22px 16px;
    border: 1.5px dashed var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--muted);
    transition:
      border-color 0.12s,
      background 0.12s;
  }

  .drop.dragging {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 8%, var(--surface));
    color: var(--accent);
  }

  .drop-title {
    margin: 6px 0 0;
    font-weight: 600;
    font-size: 15px;
    color: var(--text);
  }

  .drop-hint {
    margin: 0;
    font-size: 12.5px;
    max-width: 440px;
  }

  .drop-actions {
    display: flex;
    gap: 8px;
    margin-top: 10px;
  }

  .inputs-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 12.5px;
  }

  .chips {
    list-style: none;
    margin: 4px 0 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    max-height: 72px;
    overflow-y: auto;
  }

  .chips li {
    display: flex;
    align-items: center;
    gap: 2px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 99px;
    padding: 2px 4px 2px 11px;
    font-size: 12.5px;
    max-width: 260px;
  }

  .chips span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chips button {
    padding: 0 6px;
    font-size: 15px;
    line-height: 1;
  }

  .results {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 4px 12px;
  }

  .empty {
    text-align: center;
    margin: 28px 0;
  }

  .error {
    color: var(--danger);
    margin: 8px 0;
    font-size: 13px;
  }

  .error-inline {
    color: var(--danger);
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 12px 0 16px;
  }

  .status {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
    flex: 1;
    font-size: 13px;
  }

  .actions {
    display: flex;
    gap: 8px;
  }

  .actions .primary {
    min-width: 120px;
  }
</style>
