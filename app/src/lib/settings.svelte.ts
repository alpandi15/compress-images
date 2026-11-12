// Settings loaded from Rust, auto-saved (debounced) whenever the UI changes them.

import { api, PRESET_QUALITY, validateOptions, type Preset, type PresetChoice, type Settings } from "./api";
import { setLanguage } from "./i18n.svelte";

const SAVE_DELAY_MS = 400;

class SettingsStore {
  value = $state<Settings | null>(null);
  saveError = $state<string | null>(null);
  invalid = $derived(this.value ? validateOptions(this.value.options) : null);

  #timer: ReturnType<typeof setTimeout> | null = null;

  async load(): Promise<Settings> {
    const s = await api.getSettings();
    this.value = s;
    setLanguage(s.language);
    return s;
  }

  setPreset(preset: PresetChoice): void {
    if (!this.value) return;
    this.value.preset = preset;
    if (preset !== "custom") this.value.options.quality = PRESET_QUALITY[preset];
    this.changed();
  }

  setQuality(q: number): void {
    if (!this.value) return;
    this.value.options.quality = q;
    const match = (Object.keys(PRESET_QUALITY) as Preset[]).find((p) => PRESET_QUALITY[p] === q);
    this.value.preset = match ?? "custom";
    this.changed();
  }

  /** Call after mutating `value`; persists once input settles and the options are valid. */
  changed(): void {
    if (!this.value) return;
    setLanguage(this.value.language);
    if (this.#timer) clearTimeout(this.#timer);
    if (this.invalid) return;
    const snapshot = $state.snapshot(this.value) as Settings;
    this.#timer = setTimeout(() => {
      api.saveSettings(snapshot).then(
        () => (this.saveError = null),
        (e: unknown) => (this.saveError = String(e)),
      );
    }, SAVE_DELAY_MS);
  }
}

export const settings = new SettingsStore();
