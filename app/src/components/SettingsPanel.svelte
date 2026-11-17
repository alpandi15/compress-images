<script lang="ts">
  import type { ConflictPolicy, Language, OutputFormat, PresetChoice } from "../lib/api";
  import { t, type MessageKey } from "../lib/i18n.svelte";
  import { settings } from "../lib/settings.svelte";
  import IntegrationCard from "./IntegrationCard.svelte";

  let { disabled = false }: { disabled?: boolean } = $props();

  const presets: PresetChoice[] = ["light", "balanced", "aggressive", "custom"];
  const formats: OutputFormat[] = ["same", "jpeg", "png", "webp", "avif"];
  const formatLabel: Record<OutputFormat, string> = {
    same: "",
    jpeg: "JPEG",
    png: "PNG",
    webp: "WebP",
    avif: "AVIF",
  };
  const conflicts: ConflictPolicy[] = ["overwrite", "skip", "rename"];
  const languages: Language[] = ["auto", "en", "id"];

  const s = $derived(settings.value);

  function presetKey(p: PresetChoice): MessageKey {
    return `settings.preset.${p}` as MessageKey;
  }

  function sizeInput(e: Event, key: "max_width" | "max_height") {
    if (!s) return;
    const raw = (e.currentTarget as HTMLInputElement).value.trim();
    const n = Number.parseInt(raw, 10);
    s.options.resize[key] = raw === "" || Number.isNaN(n) ? null : n;
    settings.changed();
  }
</script>

{#if s}
  <section class="panel" aria-labelledby="settings-title">
    <h2 id="settings-title">{t("settings.title")}</h2>

    <fieldset {disabled}>
      <div class="field">
        <div class="label-row">
          <span class="label">{t("settings.quality")}</span>
          <span class="value">{s.options.quality}</span>
        </div>
        <div class="segmented" role="radiogroup" aria-label={t("settings.quality")}>
          {#each presets as p (p)}
            <button
              type="button"
              role="radio"
              aria-checked={s.preset === p}
              class:active={s.preset === p}
              onclick={() => settings.setPreset(p)}>{t(presetKey(p))}</button
            >
          {/each}
        </div>
        <input
          type="range"
          min="1"
          max="100"
          value={s.options.quality}
          aria-label={t("settings.quality")}
          oninput={(e) => settings.setQuality(Number(e.currentTarget.value))}
        />
        <span class="hint">{t("settings.qualityHint")}</span>
      </div>

      <label class="field">
        <span class="label">{t("settings.format")}</span>
        <select bind:value={s.options.output_format} onchange={() => settings.changed()}>
          {#each formats as f (f)}
            <option value={f}>{f === "same" ? t("settings.format.same") : formatLabel[f]}</option>
          {/each}
        </select>
      </label>

      <div class="field">
        <span class="label">{t("settings.resize")}</span>
        <div class="pair">
          <input
            type="number"
            min="1"
            placeholder={t("settings.width")}
            aria-label={t("settings.width")}
            aria-invalid={settings.invalid === "resize"}
            value={s.options.resize.max_width ?? ""}
            oninput={(e) => sizeInput(e, "max_width")}
          />
          <span class="muted">×</span>
          <input
            type="number"
            min="1"
            placeholder={t("settings.height")}
            aria-label={t("settings.height")}
            aria-invalid={settings.invalid === "resize"}
            value={s.options.resize.max_height ?? ""}
            oninput={(e) => sizeInput(e, "max_height")}
          />
        </div>
        {#if settings.invalid === "resize"}
          <span class="error">{t("settings.invalid.resize")}</span>
        {/if}
      </div>

      <label class="check">
        <input type="checkbox" bind:checked={s.options.recursive} onchange={() => settings.changed()} />
        {t("settings.recursive")}
      </label>
      <label class="check">
        <input type="checkbox" bind:checked={s.options.keep_metadata} onchange={() => settings.changed()} />
        {t("settings.keepMetadata")}
      </label>

      <label class="field">
        <span class="label">{t("settings.outDir")}</span>
        <input
          type="text"
          spellcheck="false"
          aria-invalid={settings.invalid === "out_dir_name"}
          bind:value={s.options.out_dir_name}
          oninput={() => settings.changed()}
        />
        {#if settings.invalid === "out_dir_name"}
          <span class="error">{t("settings.invalid.out_dir_name")}</span>
        {/if}
      </label>

      <label class="field">
        <span class="label">{t("settings.conflict")}</span>
        <select bind:value={s.options.on_conflict} onchange={() => settings.changed()}>
          {#each conflicts as c (c)}
            <option value={c}>{t(`settings.conflict.${c}` as MessageKey)}</option>
          {/each}
        </select>
      </label>
    </fieldset>

    <div class="bottom">
      <IntegrationCard />
    </div>

    <label class="field lang">
      <span class="label">{t("settings.language")}</span>
      <select bind:value={s.language} onchange={() => settings.changed()}>
        {#each languages as l (l)}
          <option value={l}>{l === "auto" ? t("settings.language.auto") : l === "en" ? "English" : "Bahasa Indonesia"}</option>
        {/each}
      </select>
    </label>

    {#if settings.saveError}
      <p class="error" role="alert">{t("settings.saveFailed", { error: settings.saveError })}</p>
    {/if}
  </section>
{/if}

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 18px;
    background: var(--surface);
    border-left: 1px solid var(--border);
    overflow-y: auto;
    height: 100%;
  }

  h2 {
    margin: 0;
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }

  fieldset {
    border: 0;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .label {
    font-weight: 600;
    font-size: 13px;
  }

  .label-row {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
  }

  .value {
    font-variant-numeric: tabular-nums;
    font-weight: 600;
    color: var(--accent);
  }

  .hint {
    font-size: 12px;
    color: var(--muted);
  }

  .segmented {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    background: var(--surface-2);
    border-radius: 9px;
    padding: 3px;
    gap: 2px;
  }

  .segmented button {
    border: 0;
    background: transparent;
    padding: 5px 2px;
    font-size: 12px;
    border-radius: 7px;
    color: var(--muted);
  }

  .segmented button.active {
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--shadow);
  }

  .pair {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: 6px;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .bottom {
    margin-top: auto;
  }

  .lang {
    padding-top: 12px;
    border-top: 1px solid var(--border);
  }

  .error {
    color: var(--danger);
    font-size: 12px;
    margin: 0;
  }
</style>
