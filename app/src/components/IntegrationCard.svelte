<script lang="ts">
  // Context-menu integration (SPEC §5): shows whether the right-click entry is installed for
  // this OS and lets the user add, repair or remove it.
  import { onMount } from "svelte";
  import { api, uiLog, type IntegrationStatus } from "../lib/api";
  import { t, type MessageKey } from "../lib/i18n.svelte";

  let status = $state<IntegrationStatus | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);

  const platform = navigator.userAgent.includes("Mac")
    ? "macos"
    : navigator.userAgent.includes("Windows")
      ? "windows"
      : "linux";

  async function run(action: () => Promise<IntegrationStatus>) {
    busy = true;
    error = null;
    try {
      status = await action();
    } catch (e) {
      error = String(e);
      uiLog("error", `context-menu integration: ${error}`);
    } finally {
      busy = false;
    }
  }

  onMount(() => run(api.integrationStatus));
</script>

<div class="card">
  <span class="label">{t("integration.title")}</span>
  {#if status && !status.supported}
    <p class="muted">{t("integration.unsupported")}</p>
  {:else if status}
    {@const targets = status.targets.join(", ")}
    <p class:ok={status.installed && !status.outdated} class="state">
      {status.installed ? t("integration.installed", { targets }) : t("integration.notInstalled", { targets })}
    </p>
    {#if status.outdated}
      <p class="warn">{t("integration.outdated")}</p>
    {/if}
    <div class="row">
      {#if !status.installed || status.outdated}
        <button type="button" disabled={busy} onclick={() => run(api.integrationInstall)}>
          {status.outdated ? t("integration.reinstall") : t("integration.install")}
        </button>
      {/if}
      {#if status.installed}
        <!-- Right-aligned so a second click on "Add" can't land on "Remove" once it appears. -->
        <button type="button" class="ghost remove" disabled={busy} onclick={() => run(api.integrationUninstall)}>
          {t("integration.uninstall")}
        </button>
      {/if}
    </div>
    {#if status.installed && !status.outdated}
      <p class="hint">{t(`integration.hint.${platform}` as MessageKey)}</p>
    {/if}
  {/if}
  {#if error}
    <p class="error" role="alert">{t("integration.failed", { error })}</p>
  {/if}
</div>

<style>
  .card {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 12px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface-2);
  }

  .label {
    font-weight: 600;
    font-size: 13px;
  }

  p {
    margin: 0;
    font-size: 12.5px;
  }

  .state {
    color: var(--muted);
  }

  .state.ok {
    color: var(--success);
  }

  .warn {
    color: var(--warning);
  }

  .error {
    color: var(--danger);
  }

  .hint {
    color: var(--muted);
    font-size: 12px;
  }

  .row {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }

  .row button {
    padding: 5px 12px;
    font-size: 12.5px;
  }

  .row .remove {
    margin-left: auto;
  }
</style>
