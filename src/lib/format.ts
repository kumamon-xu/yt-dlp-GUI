import type { FormatInfo } from "./ytdlp";

export interface FormatOption {
  formatId: string;
  /** 主标签：1080p / 720p / 音频 m4a / 视频流 */
  label: string;
  /** 副标签：mp4 · 30fps · HDR · 128.5 MB */
  sub: string[];
}

/** 清晰度排序：视频按分辨率降序、文件大者优先；纯音频最后 */
export function formatOptions(formats: FormatInfo[]): FormatOption[] {
  const seen = new Set<string>();
  const uniq = formats.filter((f) => {
    if (seen.has(f.formatId)) return false;
    seen.add(f.formatId);
    return true;
  });

  const isAudioOnly = (f: FormatInfo) => !f.vcodec;
  const height = (f: FormatInfo) => {
    const m = f.resolution?.match(/(\d+)x(\d+)/);
    return m ? parseInt(m[2], 10) : 0;
  };
  const size = (f: FormatInfo) => f.filesize ?? f.filesizeApprox ?? 0;

  return uniq
    .sort((a, b) => {
      const aa = isAudioOnly(a);
      const ba = isAudioOnly(b);
      if (aa !== ba) return aa ? 1 : -1;
      const h = height(b) - height(a);
      if (h !== 0) return h;
      return size(b) - size(a);
    })
    .slice(0, 40)
    .map((f) => {
      const label = isAudioOnly(f)
        ? `音频 ${f.acodec ?? f.ext ?? ""}`
        : f.resolution ?? "视频流";
      const sub: string[] = [];
      if (f.ext) sub.push(f.ext);
      if (f.fps) sub.push(`${Math.round(f.fps)}fps`);
      if (f.dynamicRange) sub.push(f.dynamicRange);
      const s = size(f);
      if (s > 0) sub.push(fmtSize(s));
      if (f.note) sub.push(f.note);
      return { formatId: f.formatId, label, sub };
    });
}

export function fmtDuration(sec?: number | null): string {
  if (sec == null || !isFinite(sec)) return "";
  const s = Math.round(sec);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

/** yt-dlp 的 filesize 单位为字节 */
export function fmtSize(bytes: number): string {
  const v = bytes;
  if (v >= 1024 ** 3) return `${(v / 1024 ** 3).toFixed(2)} GB`;
  if (v >= 1024 ** 2) return `${(v / 1024 ** 2).toFixed(1)} MB`;
  if (v >= 1024) return `${(v / 1024).toFixed(0)} KB`;
  return `${Math.round(v)} B`;
}
