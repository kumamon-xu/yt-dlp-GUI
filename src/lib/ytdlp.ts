import { invoke } from "@tauri-apps/api/core";

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
export const getFfmpegPath = () => invoke<string | null>("ffmpeg_path");
export const getInfo = (url: string, enginePath: string | null = null) =>
  invoke<VideoInfo>("get_info", { url, enginePath });

export type TaskStatus =
  | "queued"
  | "downloading"
  | "postprocess"
  | "paused"
  | "done"
  | "failed"
  | "canceled";

export interface NewTask {
  url: string;
  preset: string;
  customFormat?: string;
  audioQuality?: string;
  mergeFormat?: string;
  outDir?: string;
  outTemplate?: string;
  concurrentFragments?: number;
  limitRate?: string;
  cookiesBrowser?: string;
  cookiesFile?: string;
  proxy?: string;
  embedThumbnail?: boolean;
  embedMetadata?: boolean;
  writeSubs?: boolean;
  subLangs?: string;
  embedSubs?: boolean;
  sponsorblock?: boolean;
  noPlaylist?: boolean;
  playlistItems?: string;
  resume?: boolean;
  skipDownload?: boolean;
  writeThumbnail?: boolean;
  convertSubs?: string;
  writeInfoJson?: boolean;
}

export interface TaskPayload {
  id: string;
  url: string;
  title: string | null;
  status: TaskStatus;
  downloaded: number;
  total: number;
  speed: number;
  eta: number;
  filePath: string | null;
  error: string | null;
  request: NewTask;
}

export interface GlobalSettings {
  defaultPreset: string;
  outDir: string;
  outTemplate: string;
  concurrentFragments: number;
  maxConcurrentTasks: number;
  limitRate: string | null;
  cookiesBrowser: string | null;
  cookiesFile: string | null;
  proxy: string | null;
  enginePath: string | null;
  ffmpegPath: string | null;
  mergeFormat: string;
}

export const startTask = (id: string, task: NewTask) => invoke<void>("start_task", { id, task });
export const cancelTask = (id: string) => invoke<void>("cancel_task", { id });
export const pauseTask = (id: string) => invoke<void>("pause_task", { id });
export const resumeTask = (id: string) => invoke<void>("resume_task", { id });
export const removeTask = (id: string) => invoke<void>("remove_task", { id });
export const listTasks = () => invoke<TaskPayload[]>("list_tasks");
export const openFolder = (path: string) => invoke<void>("open_folder", { path });
export const buildCommand = (task: NewTask) => invoke<string>("build_command", { task });
export const loadSettings = () => invoke<GlobalSettings>("load_settings");
export const saveSettings = (settings: GlobalSettings) => invoke<void>("save_settings", { settings });
export const pickDir = () => invoke<string | null>("pick_dir");
export const pickFile = () => invoke<string | null>("pick_file");
export const updateEngine = () => invoke<string>("update_engine");

export function toolError(reason: unknown): ToolStatus {
  return {
    available: false,
    path: null,
    version: null,
    error: reason instanceof Error ? reason.message : String(reason),
  };
}

export function splitUrls(raw: string): string[] {
  return raw
    .split(/[\s,]+/)
    .map((s) => s.trim())
    .filter((s) => /^https?:\/\/\S+$/i.test(s));
}
