// Reactive view of one compression job, driven entirely by events from the Rust side.

import type { UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { api, JOB_EVENT, type JobMessage, type Options, type Outcome, type SkipReason, type Summary } from "./api";

export type RowState =
  | { kind: "pending" }
  | { kind: "working" }
  | { kind: "planned_skip"; reason: SkipReason }
  | { kind: "done"; outcome: Outcome };

export interface Row {
  index: number;
  source: string;
  destination: string | null;
  size: number;
  state: RowState;
}

export type Phase = "idle" | "running" | "finished";

export class Job {
  phase = $state<Phase>("idle");
  rows = $state<Row[]>([]);
  inputErrors = $state<{ path: string; error: string }[]>([]);
  summary = $state<Summary | null>(null);
  current = $state<string | null>(null);
  startError = $state<string | null>(null);

  /** Files that will actually be encoded (excludes planned skips). */
  encodeTotal = $derived(this.rows.filter((r) => r.state.kind !== "planned_skip").length);
  finishedCount = $derived(
    this.rows.filter((r) => r.state.kind === "done" && r.state.outcome.status !== "skipped").length,
  );
  progress = $derived(this.encodeTotal === 0 ? 0 : this.finishedCount / this.encodeTotal);
  bytesSaved = $derived(
    this.rows.reduce((acc, r) => {
      if (r.state.kind === "done" && r.state.outcome.status === "compressed") {
        return acc + (r.state.outcome.before - r.state.outcome.after);
      }
      return acc;
    }, 0),
  );
  /** First written output, used by "Open folder". */
  firstOutput = $derived(
    this.rows.find(
      (r) =>
        r.state.kind === "done" &&
        (r.state.outcome.status === "compressed" || r.state.outcome.status === "kept_original"),
    )?.destination ?? null,
  );

  #unlisten: UnlistenFn | null = null;

  async start(paths: string[], options: Options): Promise<boolean> {
    if (this.phase === "running") return false;
    await this.#listen();
    this.rows = [];
    this.inputErrors = [];
    this.summary = null;
    this.current = null;
    this.startError = null;
    this.phase = "running";
    try {
      await api.startJob(paths, options);
      return true;
    } catch (e) {
      this.phase = "idle";
      this.startError = String(e);
      return false;
    }
  }

  cancel(): Promise<void> {
    return api.cancelJob();
  }

  reset(): void {
    if (this.phase === "running") return;
    this.phase = "idle";
    this.rows = [];
    this.inputErrors = [];
    this.summary = null;
    this.startError = null;
  }

  dispose(): void {
    this.#unlisten?.();
    this.#unlisten = null;
  }

  async #listen(): Promise<void> {
    if (this.#unlisten) return;
    // Window-scoped: events for a job started in another window never show up here.
    this.#unlisten = await getCurrentWebviewWindow().listen<JobMessage>(JOB_EVENT, (e) =>
      this.#handle(e.payload),
    );
  }

  #handle(msg: JobMessage): void {
    switch (msg.kind) {
      case "input_error":
        this.inputErrors.push({ path: msg.path, error: msg.error });
        break;
      case "planned":
        this.rows = msg.items.map((i) => ({
          index: i.index,
          source: i.source,
          destination: i.destination,
          size: i.size,
          state: i.skip ? { kind: "planned_skip", reason: i.skip } : { kind: "pending" },
        }));
        break;
      case "core": {
        const ev = msg.event;
        if (ev.event === "file_started") {
          const row = this.rows[ev.index];
          if (row) row.state = { kind: "working" };
          this.current = ev.source;
        } else if (ev.event === "file_done") {
          const row = this.rows[ev.index];
          if (row && row.state.kind !== "planned_skip") {
            row.state = { kind: "done", outcome: ev.outcome };
            row.destination = ev.destination ?? row.destination;
          }
        } else if (ev.event === "finished") {
          this.summary = ev.summary;
          this.current = null;
          this.phase = "finished";
        }
        break;
      }
    }
  }
}
