import { create } from "zustand";
import { checkEngine, checkFfmpeg, toolError, type ToolStatus } from "./lib/ytdlp";

interface AppState {
  engine: ToolStatus | null;
  ffmpeg: ToolStatus | null;
  checking: boolean;
  refresh: () => Promise<void>;
}

export const useAppStore = create<AppState>((set) => ({
  engine: null,
  ffmpeg: null,
  checking: false,
  refresh: async () => {
    set({ checking: true });
    const [engine, ffmpeg] = await Promise.allSettled([checkEngine(), checkFfmpeg()]);
    set({
      checking: false,
      engine: engine.status === "fulfilled" ? engine.value : toolError(engine.reason),
      ffmpeg: ffmpeg.status === "fulfilled" ? ffmpeg.value : toolError(ffmpeg.reason),
    });
  },
}));
