import type { FormatInfo } from "./ytdlp";

export interface FormatOption {
  formatId: string;
  label: string;
  sub: string[];
  vcodec: string | null;
  acodec: string | null;
  resolution: string | null;
  height: number;
}

export function formatHeight(resolution: string | null): number {
  const m = resolution?.match(/(\d+)x(\d+)/);
  return m ? parseInt(m[2], 10) : 0;
}

export function formatOptions(formats: FormatInfo[]): FormatOption[] {
  const seen = new Set<string>();
  const uniq = formats.filter((f) => {
    if (!f.formatId || seen.has(f.formatId)) return false;
    seen.add(f.formatId);
    return true;
  });

  const isAudioOnly = (f: FormatInfo) => !f.vcodec;
  const size = (f: FormatInfo) => f.filesize ?? f.filesizeApprox ?? 0;

  return uniq
    .sort((a, b) => {
      const aa = isAudioOnly(a);
      const ba = isAudioOnly(b);
      if (aa !== ba) return aa ? 1 : -1;
      const h = formatHeight(b.resolution) - formatHeight(a.resolution);
      if (h !== 0) return h;
      return size(b) - size(a);
    })
    .slice(0, 60)
    .map((f) => {
      const height = formatHeight(f.resolution);
      const label = isAudioOnly(f)
        ? `音频 ${f.acodec ?? f.ext ?? ""}`
        : height > 0
          ? `${height}p`
          : "视频流";
      const sub: string[] = [];
      if (f.vcodec) sub.push(f.vcodec);
      if (f.acodec && !f.vcodec) sub.push(f.acodec);
      if (f.ext) sub.push(f.ext);
      if (f.fps) sub.push(`${Math.round(f.fps)}fps`);
      if (f.dynamicRange) sub.push(f.dynamicRange);
      const s = size(f);
      if (s > 0) sub.push(fmtSize(s));
      if (f.note) sub.push(f.note);
      return {
        formatId: f.formatId,
        label,
        sub,
        vcodec: f.vcodec,
        acodec: f.acodec,
        resolution: f.resolution,
        height,
      };
    });
}

/** 仅视频补 +ba，避免无声文件 */
export function customFormatFromSelection(o: FormatOption | null): { preset: string; customFormat?: string } {
  if (!o) return { preset: "best" };
  if (o.vcodec && !o.acodec) {
    return { preset: "custom", customFormat: `${o.formatId}+ba/${o.formatId}/b` };
  }
  return { preset: "custom", customFormat: o.formatId };
}

export function fmtDuration(sec?: number | null): string {
  if (sec == null || !isFinite(sec)) return "";
  const s = Math.round(sec);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

export function fmtSpeed(n: number | null | undefined): string {
  if (n == null || !isFinite(n) || n <= 0) return "—";
  const mb = n / 1024 / 1024;
  return mb >= 1 ? `${mb.toFixed(1)} MB/s` : `${Math.round(n / 1024)} KB/s`;
}

export function fmtEta(sec: number | null | undefined): string {
  if (sec == null || !isFinite(sec) || sec <= 0) return "—";
  const s = Math.round(sec);
  const m = Math.floor(s / 60);
  return `${String(m).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

export function fmtSize(bytes: number): string {
  const v = bytes;
  if (v >= 1024 ** 3) return `${(v / 1024 ** 3).toFixed(2)} GB`;
  if (v >= 1024 ** 2) return `${(v / 1024 ** 2).toFixed(1)} MB`;
  if (v >= 1024) return `${(v / 1024).toFixed(0)} KB`;
  return `${Math.round(v)} B`;
}
