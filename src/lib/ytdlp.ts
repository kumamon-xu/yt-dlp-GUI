import { invoke } from "@tauri-apps/api/core";

/** Rust ToolStatus 的 TS 镜像（camelCase） */
export interface ToolStatus {
  available: boolean;
  path: string | null;
  version: string | null;
  error: string | null;
}

export interface FormatInfo {
  formatId: string;
  ext: string | null;
  resolution: string | null;
  vcodec: string | null;
  acodec: string | null;
  filesize: number | null;
  filesizeApprox: number | null;
  fps: number | null;
  dynamicRange: string | null;
  protocol: string | null;
  container: string | null;
  note: string | null;
  language: string | null;
}

export interface PlaylistItem {
  id: string;
  title: string;
  thumbnail: string | null;
  duration: number | null;
  webpageUrl: string | null;
  channel: string | null;
}

export interface VideoInfo {
  id: string;
  title: string;
  thumbnail: string | null;
  duration: number | null;
  uploader: string | null;
  description: string | null;
  webpageUrl: string | null;
  isPlaylist: boolean;
  formats: FormatInfo[];
  playlist: PlaylistItem[] | null;
  playlistTitle: string | null;
  playlistCount: number | null;
}

export const checkEngine = () => invoke<ToolStatus>("check_engine");
export const checkFfmpeg = () => invoke<ToolStatus>("check_ffmpeg");
export const checkJsRuntime = () => invoke<ToolStatus>("check_js_runtime");
export const getEnginePath = () => invoke<string | null>("engine_path");

/** URL 元数据预览（不下载） */
export const getInfo = (url: string) => invoke<VideoInfo>("get_info", { url, enginePath: null });

/** invoke 失败时归一化为 ToolStatus */
export function toolError(reason: unknown): ToolStatus {
  return {
    available: false,
    path: null,
    version: null,
    error: reason instanceof Error ? reason.message : String(reason),
  };
}
