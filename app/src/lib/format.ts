export function humanBytes(bytes: number): string {
  const units = ["B", "KB", "MB", "GB"];
  let v = bytes;
  let unit = 0;
  while (v >= 1024 && unit < units.length - 1) {
    v /= 1024;
    unit++;
  }
  return unit === 0 ? `${bytes} B` : `${v.toFixed(1)} ${units[unit]}`;
}

export function percentChange(before: number, after: number): string {
  if (before === 0) return "0%";
  const p = (after / before - 1) * 100;
  return p <= 0 ? `−${Math.round(-p)}%` : `+${Math.round(p)}%`;
}

const SEP = /[\\/]/;

export function baseName(path: string): string {
  const parts = path.split(SEP);
  return parts[parts.length - 1] || path;
}

/** Path shown in lists: relative to the selected folder that contains it, else the file name. */
export function displayPath(path: string, roots: string[]): string {
  for (const root of roots) {
    const r = root.replace(/[\\/]+$/, "");
    if (path.length > r.length && path.startsWith(r) && SEP.test(path.charAt(r.length))) {
      return `${baseName(r)}/${path.slice(r.length + 1).replaceAll("\\", "/")}`;
    }
  }
  return baseName(path);
}
