<script lang="ts">
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import MainView from "./components/MainView.svelte";
  import QuickView from "./components/QuickView.svelte";
  import { settings } from "./lib/settings.svelte";

  // The Rust side opens the quick-mode window with the label "quick".
  const isQuick = getCurrentWebviewWindow().label === "quick";
  const ready = settings.load();
</script>

{#await ready then}
  {#if isQuick}
    <QuickView />
  {:else}
    <MainView />
  {/if}
{:catch error}
  <p class="fatal">{String(error)}</p>
{/await}

<style>
  .fatal {
    padding: 24px;
    color: var(--danger);
  }
</style>
