import { invoke } from "@tauri-apps/api/core";

export type ToolSource = "override" | "managed" | "bundled" | "path";

export interface ToolStatus {
  available: boolean;
  path: string | null;
  version: string | null;
  error: string | null;
  source?: ToolSource | null;
}

export interface EngineUpdateResult {
  updated: boolean;
  oldVersion: string | null;
  newVersion: string | null;
  message: string;
  source: ToolSource;
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
export const getInfo = (url: string, enginePath: string | null = null, flat = true) =>
  invoke<VideoInfo>("get_info", { url, enginePath, flat });

export type TaskStatus =
  | "queued"
  | "starting"
  | "downloading"
  | "postprocess"
  | "paused"
  | "done"
  | "failed"
  | "canceled";

export type TaskKind = "video" | "audio" | "subtitles" | "thumbnail" | "metadata";
export type ProxySource = "global" | "explicit" | "none";

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
  proxySource?: ProxySource;
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
  kind?: TaskKind;
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
  outputFiles?: string[];
  error: string | null;
  errorCode?: string | null;
  request: NewTask;
  kind?: TaskKind;
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
export const startTasks = (items: { id: string; task: NewTask }[]) =>
  invoke<void>("start_tasks", { items });
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
export const updateEngine = () => invoke<EngineUpdateResult>("update_engine");

export function toolError(reason: unknown): ToolStatus {
  return {
    available: false,
    path: null,
    version: null,
    error: reason instanceof Error ? reason.message : String(reason),
    source: null,
  };
}

export function splitUrls(raw: string): string[] {
  return raw
    .split(/[\s,]+/)
    .map((s) => s.trim())
    .filter((s) => /^https?:\/\/\S+$/i.test(s));
}

export function resolveDownloadPreset(
  extraPreset: string | undefined,
  sessionPreset: string,
  defaultPreset: string,
): string {
  if (extraPreset && extraPreset.trim()) return extraPreset;
  if (sessionPreset && sessionPreset.trim()) return sessionPreset;
  return defaultPreset || "mp4";
}

export function settingsDraftDirty(a: GlobalSettings, b: GlobalSettings): boolean {
  return JSON.stringify(a) !== JSON.stringify(b);
}

export function retryTaskRequest(row: { url: string; request: NewTask }): NewTask {
  return { ...row.request, url: row.url, resume: true };
}

export function isCurrentToken(token: number, latest: number): boolean {
  return token === latest;
}

export function canCancelStatus(status: TaskStatus): boolean {
  return status === "queued" || status === "starting" || status === "downloading" || status === "postprocess";
}

export function canRemoveStatus(status: TaskStatus): boolean {
  return status === "queued" || status === "paused" || status === "done" || status === "failed" || status === "canceled";
}

export function acceptTaskUpdate(knownIds: Set<string>, id: string): boolean {
  return knownIds.has(id);
}

export function compressPlaylistItems(indices: number[]): string {
  const nums = [...new Set(indices.filter((n) => n > 0))].sort((a, b) => a - b);
  if (!nums.length) return "";
  const parts: string[] = [];
  let start = nums[0];
  let prev = nums[0];
  for (let i = 1; i < nums.length; i++) {
    const n = nums[i];
    if (n === prev + 1) {
      prev = n;
      continue;
    }
    parts.push(start === prev ? String(start) : `${start}-${prev}`);
    start = n;
    prev = n;
  }
  parts.push(start === prev ? String(start) : `${start}-${prev}`);
  return parts.join(",");
}

export function playlistIsTruncated(loadedCount: number, totalCount: number | null): boolean {
  return totalCount == null || totalCount > loadedCount;
}

export function playlistItemsForSelection(
  selected: number[],
  loadedCount: number,
  totalCount: number | null,
): string | undefined {
  const normalized = [...new Set(selected.filter((n) => Number.isInteger(n) && n >= 1 && n <= loadedCount))]
    .sort((a, b) => a - b);
  const fullyLoaded = totalCount != null && totalCount <= loadedCount;
  if (fullyLoaded && normalized.length === loadedCount) return undefined;
  return compressPlaylistItems(normalized);
}

export function redactUserinfo(s: string): string {
  const i = s.indexOf("://");
  if (i < 0) return s;
  const rest = s.slice(i + 3);
  const at = rest.indexOf("@");
  if (at < 0) return s;
  const creds = rest.slice(0, at);
  const colon = creds.indexOf(":");
  if (colon < 0) return s;
  return `${s.slice(0, i + 3)}${creds.slice(0, colon)}:***${rest.slice(at)}`;
}

export function toolboxTask(
  url: string,
  kind: TaskKind,
  net: Pick<NewTask, "outDir" | "proxy" | "cookiesBrowser" | "cookiesFile">,
): NewTask {
  return {
    url,
    preset: "mp4",
    kind,
    noPlaylist: true,
    skipDownload: true,
    writeSubs: kind === "subtitles",
    convertSubs: kind === "subtitles" ? "srt" : undefined,
    writeThumbnail: kind === "thumbnail",
    writeInfoJson: kind === "metadata",
    outDir: net.outDir,
    proxy: net.proxy,
    proxySource: net.proxy ? "global" : "none",
    cookiesBrowser: net.cookiesBrowser,
    cookiesFile: net.cookiesFile,
  };
}
