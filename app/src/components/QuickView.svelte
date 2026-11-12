<script lang="ts">
  // Small always-on-top window for context-menu launches (SPEC §4.3): uses the saved
  // settings, shows progress, then the summary, and closes itself after 10 s if untouched.
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { onDestroy, onMount } from "svelte";
  import { api, QUICK_BATCH_EVENT, uiLog } from "../lib/api";
  import { baseName, humanBytes, percentChange } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import { Job } from "../lib/job.svelte";
  import { settings } from "../lib/settings.svelte";

  const AUTO_CLOSE_S = 10;
  const RETRY_MS = 1000;

  const job = new Job();
  const win = getCurrentWebviewWindow();
  let waiting = $state(false);
  let countdown = $state<number | null>(null);
  let pinned = $state(false); // any interaction cancels auto-close
  let timer: ReturnType<typeof setInterval> | null = null;
  let retry: ReturnType<typeof setTimeout> | null = null;
  let unlistenBatch: UnlistenFn | null = null;

  async function runNextBatch() {
    if (job.phase === "running" || !settings.value) return;
    const paths = await api.takeQuickBatch();
    uiLog("debug", `quick: took batch of ${paths.length}`);
    if (paths.length === 0) return;
    stopCountdown();
    await startWithRetry(paths);
  }

  async function startWithRetry(paths: string[]) {
    if (!settings.value) return;
    const ok = await job.start(paths, $state.snapshot(settings.value.options));
    // Only one job runs at a time; if the main window is busy, wait for it.
    if (!ok && (await api.isJobRunning())) {
      waiting = true;
      job.startError = null;
      retry = setTimeout(() => void startWithRetry(paths), RETRY_MS);
      return;
    }
    waiting = false;
  }

  function stopCountdown() {
    if (timer) clearInterval(timer);
    timer = null;
    countdown = null;
  }

  function startCountdown() {
    uiLog("debug", `quick: job finished, auto-close ${pinned ? "disabled (user interacted)" : "starting"}`);
    if (pinned) return;
    countdown = AUTO_CLOSE_S;
    timer = setInterval(() => {
      if (countdown === null) return;
      countdown -= 1;
      if (countdown <= 0) {
        stopCountdown();
        uiLog("debug", "quick: auto-closing");
        win.close().catch((e: unknown) => uiLog("error", `quick: close failed: ${String(e)}`));
      }
    }, 1000);
  }

  function pin() {
    pinned = true;
    stopCountdown();
  }

  $effect(() => {
    if (job.phase === "finished") {
      // A batch may have arrived while we were busy.
      void api.takeQuickBatch().then((paths) => {
        if (paths.length > 0) void startWithRetry(paths);
        else startCountdown();
      });
    }
  });

  onMount(async () => {
    unlistenBatch = await win.listen(QUICK_BATCH_EVENT, () => void runNextBatch());
    await runNextBatch();
  });

  onDestroy(() => {
    stopCountdown();
    if (retry) clearTimeout(retry);
    unlistenBatch?.();
    job.dispose();
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="quick" onpointerdown={pin} onkeydown={pin}>
  {#if job.phase === "finished" && job.summary}
    {@const s = job.summary}
    <h1>{s.cancelled ? t("summary.cancelled") : t("summary.done")}</h1>
    {#if s.compressed + s.kept_original > 0}
      <p class="big">
        {humanBytes(s.bytes_before)} → {humanBytes(s.bytes_after)}
        <span class="saved">{percentChange(s.bytes_before, s.bytes_after)}</span>
      </p>
    {/if}
    <p class="muted">
      {t("summary.line", {
        compressed: s.compressed + s.kept_original,
        before: humanBytes(s.bytes_before),
        after: humanBytes(s.bytes_after),
        percent: percentChange(s.bytes_before, s.bytes_after),
      })}
      {#if s.failed > 0}<br /><span class="error">{t("summary.failed", { n: s.failed })}</span>{/if}
      {#if s.skipped > 0}<br />{t("summary.skipped", { n: s.skipped })}{/if}
    </p>
    {#each job.inputErrors as err (err.path)}
      <p class="error small">{t("errors.input", { path: baseName(err.path), error: err.error })}</p>
    {/each}
    <div class="actions">
      <button type="button" class="ghost" onclick={() => void api.showMainWindow()}>{t("action.openApp")}</button>
      <span class="spacer"></span>
      {#if countdown !== null}<span class="muted small">{t("quick.closingIn", { s: countdown })}</span>{/if}
      {#if job.firstOutput}
        <button type="button" onclick={() => job.firstOutput && void revealItemInDir(job.firstOutput)}>
          {t("action.openFolder")}
        </button>
      {/if}
      <button type="button" class="primary" onclick={() => void win.close()}>{t("action.close")}</button>
    </div>
  {:else}
    <h1>
      {#if waiting}
        {t("quick.waiting")}
      {:else if job.encodeTotal > 0}
        {t("quick.compressing", { n: job.encodeTotal })}
      {:else}
        {t("quick.preparing")}
      {/if}
    </h1>
    <div class="progress" role="progressbar" aria-valuenow={Math.round(job.progress * 100)} aria-valuemin="0" aria-valuemax="100">
      <div style:width={`${job.progress * 100}%`}></div>
    </div>
    <p class="muted current">{job.current ? baseName(job.current) : " "}</p>
    <p class="muted">
      {t("progress.count", { done: job.finishedCount, total: job.encodeTotal })}
      {#if job.bytesSaved > 0}· −{humanBytes(job.bytesSaved)}{/if}
    </p>
    {#if job.startError}
      <p class="error small">{t("errors.start", { error: job.startError })}</p>
    {/if}
    <div class="actions">
      <span class="spacer"></span>
      <button type="button" onclick={() => job.cancel()} disabled={job.phase !== "running"}>{t("action.cancel")}</button>
    </div>
  {/if}
</div>

<style>
  .quick {
    display: flex;
    flex-direction: column;
    gap: 10px;
    height: 100%;
    padding: 20px 22px 18px;
    background: var(--surface);
  }

  h1 {
    margin: 0;
    font-size: 16px;
    font-weight: 650;
  }

  p {
    margin: 0;
    font-size: 13px;
  }

  .big {
    font-size: 20px;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
  }

  .saved {
    color: var(--success);
    margin-left: 6px;
  }

  .current {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .small {
    font-size: 12px;
  }

  .error {
    color: var(--danger);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: auto;
  }

  .spacer {
    flex: 1;
  }
</style>
