import { invoke } from "@tauri-apps/api/core";

/** Rust ToolStatus 的 TS 镜像（camelCase） */
export interface ToolStatus {
  available: boolean;
  path: string | null;
  version: string | null;
  error: string | null;
}

export const checkEngine = () => invoke<ToolStatus>("check_engine");
export const checkFfmpeg = () => invoke<ToolStatus>("check_ffmpeg");
export const getEnginePath = () => invoke<string | null>("engine_path");

/** invoke 失败时归一化为 ToolStatus */
export function toolError(reason: unknown): ToolStatus {
  return {
    available: false,
    path: null,
    version: null,
    error: reason instanceof Error ? reason.message : String(reason),
  };
}
