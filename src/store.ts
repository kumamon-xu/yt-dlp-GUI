import { create } from "zustand";
import { checkEngine, checkFfmpeg, checkJsRuntime, getInfo, toolError, type ToolStatus, type VideoInfo } from "./lib/ytdlp";

type PreviewStatus = "idle" | "loading" | "ok" | "error";

interface AppState {
  // M0: 环境
  engine: ToolStatus | null;
  ffmpeg: ToolStatus | null;
  jsRuntime: ToolStatus | null;
  checking: boolean;
  refresh: () => Promise<void>;

  // M1: 预览
  previewStatus: PreviewStatus;
  previewUrl: string;
  previewInfo: VideoInfo | null;
  previewError: string | null;
  selectedItems: string[]; // 播放列表勾选的条目 id
  selectedFormat: string | null; // 选中的 format_id
  preview: (url: string) => Promise<void>;
  toggleItem: (id: string) => void;
  setAllItems: (ids: string[], on: boolean) => void;
  selectFormat: (id: string) => void;
}

let previewToken = 0; // 防止慢请求覆盖新结果

export const useAppStore = create<AppState>((set, get) => ({
  engine: null,
  ffmpeg: null,
  jsRuntime: null,
  checking: false,
  refresh: async () => {
    set({ checking: true });
    const [engine, ffmpeg, jsRuntime] = await Promise.allSettled([
      checkEngine(),
      checkFfmpeg(),
      checkJsRuntime(),
    ]);
    set({
      checking: false,
      engine: engine.status === "fulfilled" ? engine.value : toolError(engine.reason),
      ffmpeg: ffmpeg.status === "fulfilled" ? ffmpeg.value : toolError(ffmpeg.reason),
      jsRuntime: jsRuntime.status === "fulfilled" ? jsRuntime.value : toolError(jsRuntime.reason),
    });
  },

  previewStatus: "idle",
  previewUrl: "",
  previewInfo: null,
  previewError: null,
  selectedItems: [],
  selectedFormat: null,

  preview: async (url: string) => {
    const token = ++previewToken;
    set({ previewStatus: "loading", previewUrl: url, previewError: null, selectedFormat: null });
    try {
      const info = await getInfo(url);
      if (token !== previewToken) return; // 已有更新请求
      const items = info.playlist ?? [];
      set({
        previewStatus: "ok",
        previewInfo: info,
        selectedItems: items.map((i) => i.id), // 默认全选
      });
    } catch (e) {
      if (token !== previewToken) return;
      set({
        previewStatus: "error",
        previewInfo: null,
        previewError: e instanceof Error ? e.message : String(e),
      });
    }
  },

  toggleItem: (id) => {
    const cur = get().selectedItems;
    set({
      selectedItems: cur.includes(id) ? cur.filter((x) => x !== id) : [...cur, id],
    });
  },

  setAllItems: (ids, on) => set({ selectedItems: on ? ids : [] }),

  selectFormat: (id) => set({ selectedFormat: id }),
}));
