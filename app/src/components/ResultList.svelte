<script lang="ts">
  import { displayPath, humanBytes, percentChange } from "../lib/format";
  import { t, type MessageKey } from "../lib/i18n.svelte";
  import type { Row } from "../lib/job.svelte";

  let { rows, roots }: { rows: Row[]; roots: string[] } = $props();

  type View = { icon: string; tone: string; detail: string };

  function view(row: Row): View {
    const s = row.state;
    switch (s.kind) {
      case "pending":
        return { icon: "•", tone: "muted", detail: t("status.pending") };
      case "working":
        return { icon: "◌", tone: "accent", detail: t("status.working") };
      case "planned_skip":
        return { icon: "–", tone: "muted", detail: t(`skip.${s.reason}` as MessageKey) };
      case "done": {
        const o = s.outcome;
        switch (o.status) {
          case "compressed":
            return {
              icon: "✓",
              tone: o.larger_than_source ? "warning" : "success",
              detail:
                `${humanBytes(o.before)} → ${humanBytes(o.after)} (${percentChange(o.before, o.after)})` +
                (o.larger_than_source ? ` · ${t("status.larger")}` : ""),
            };
          case "kept_original":
            return { icon: "=", tone: "success", detail: t("status.kept") };
          case "skipped":
            return { icon: "–", tone: "muted", detail: t(`skip.${o.reason}` as MessageKey) };
          case "failed":
            return { icon: "✕", tone: "danger", detail: o.error };
        }
      }
    }
  }
</script>

<ul class="list">
  {#each rows as row (row.index)}
    {@const v = view(row)}
    <li class={v.tone}>
      <span class="icon" aria-hidden="true">{v.icon}</span>
      <span class="name" title={row.source}>{displayPath(row.source, roots)}</span>
      <span class="detail" title={v.detail}>{v.detail}</span>
    </li>
  {/each}
</ul>

<style>
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  li {
    display: grid;
    grid-template-columns: 20px minmax(0, 1fr) auto;
    align-items: center;
    gap: 8px;
    padding: 7px 4px;
    border-bottom: 1px solid var(--border);
    font-size: 13px;
  }

  li:last-child {
    border-bottom: 0;
  }

  .icon {
    text-align: center;
    font-weight: 700;
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .detail {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    max-width: 320px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .success .icon {
    color: var(--success);
  }
  .warning .icon,
  .warning .detail {
    color: var(--warning);
  }
  .danger .icon,
  .danger .detail {
    color: var(--danger);
  }
  .accent .icon {
    color: var(--accent);
  }
  .muted .icon,
  .muted .name {
    color: var(--muted);
  }
</style>
