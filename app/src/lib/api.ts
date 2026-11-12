// Typed bridge to the Rust side. Shapes mirror compress-core / src-tauri serde output.

import { invoke } from "@tauri-apps/api/core";

export type OutputFormat = "same" | "jpeg" | "png" | "webp" | "avif";
export type ConflictPolicy = "overwrite" | "skip" | "rename";
export type Preset = "light" | "balanced" | "aggressive";
/** `custom` = free quality value; stored explicitly (see settings.rs). */
export type PresetChoice = Preset | "custom";
export type Language = "auto" | "en" | "id";
export type SkipReason = "unsupported" | "animated" | "multi_page" | "exists" | "cancelled";

export const PRESET_QUALITY: Record<Preset, number> = {
  light: 85,
  balanced: 70,
  aggressive: 50,
};

export interface Resize {
  max_width?: number | null;
  max_height?: number | null;
}

export interface Options {
  quality: number;
  output_format: OutputFormat;
  recursive: boolean;
  resize: Resize;
  keep_metadata: boolean;
  out_dir_name: string;
  on_conflict: ConflictPolicy;
  jobs?: number | null;
  avif_speed: number;
}

export interface Settings {
  options: Options;
  preset: PresetChoice;
  language: Language;
}

export type Outcome =
  | { status: "compressed"; before: number; after: number; larger_than_source: boolean }
  | { status: "kept_original"; size: number }
  | { status: "skipped"; reason: SkipReason }
  | { status: "failed"; error: string };

export interface Summary {
  total: number;
  compressed: number;
  kept_original: number;
  skipped: number;
  failed: number;
  bytes_before: number;
  bytes_after: number;
  elapsed_ms: number;
  cancelled: boolean;
}

export type CoreEvent =
  | { event: "started"; total: number }
  | { event: "file_started"; index: number; source: string }
  | {
      event: "file_done";
      index: number;
      source: string;
      destination: string | null;
      outcome: Outcome;
    }
  | { event: "finished"; summary: Summary };

export interface PlannedItem {
  index: number;
  source: string;
  destination: string | null;
  size: number;
  skip: SkipReason | null;
}

export type JobMessage =
  | { kind: "input_error"; path: string; error: string }
  | { kind: "planned"; items: PlannedItem[] }
  | { kind: "core"; event: CoreEvent };

export const JOB_EVENT = "job";
export const QUICK_BATCH_EVENT = "quick-batch";

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  startJob: (paths: string[], options: Options) => invoke<void>("start_job", { paths, options }),
  cancelJob: () => invoke<void>("cancel_job"),
  isJobRunning: () => invoke<boolean>("is_job_running"),
  takeQuickBatch: () => invoke<string[]>("take_quick_batch"),
  showMainWindow: () => invoke<void>("show_main_window"),
};

/** Writes to the backend log (visible with RUST_LOG=compress_images_app=debug). */
export function uiLog(level: "debug" | "warn" | "error", message: string): void {
  void invoke("log_from_ui", { level, message }).catch(() => {});
}

/// Mirrors `Options::validate` so the UI can explain problems before saving.
export function validateOptions(o: Options): "out_dir_name" | "resize" | null {
  const n = o.out_dir_name;
  if (!n || n === "." || n === ".." || /[\\/]/.test(n) || n.trim() !== n) return "out_dir_name";
  if (o.resize.max_width === 0 || o.resize.max_height === 0) return "resize";
  return null;
}
