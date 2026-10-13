import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAppStore } from "./store";
import { saveSettings } from "./lib/ytdlp";
import type { FormatInfo, GlobalSettings, VideoInfo } from "./lib/ytdlp";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue(undefined) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("./lib/ytdlp", async (importOriginal) => ({
  ...await importOriginal<typeof import("./lib/ytdlp")>(),
  saveSettings: vi.fn().mockResolvedValue(undefined),
}));

const settings = (): GlobalSettings => ({
  defaultPreset: "mp4", outDir: "C:/old", outTemplate: "%(title)s.%(ext)s",
  concurrentFragments: 4, maxConcurrentTasks: 2, limitRate: null, cookiesBrowser: null,
  cookiesFile: null, proxy: null, enginePath: null, ffmpegPath: null, mergeFormat: "mp4",
});
const initialOptions = useAppStore.getState().options;
const url = "https://test/watch";
const video: FormatInfo = {
  formatId: "v-1080", ext: "mp4", resolution: "1920x1080", vcodec: "h264", acodec: null,
  filesize: null, filesizeApprox: null, fps: 30, dynamicRange: null, protocol: "m3u8_native",
  container: null, note: null, language: null,
};
const info = (formats: FormatInfo[]): VideoInfo => ({
  id: "test", title: "test", thumbnail: null, duration: null, uploader: null,
  description: null, webpageUrl: url, isPlaylist: false, formats,
  playlist: null, playlistTitle: null, playlistCount: null,
});

beforeEach(() => {
  vi.useFakeTimers();
  vi.mocked(saveSettings).mockReset().mockResolvedValue(undefined);
  useAppStore.setState({
    settings: settings(), options: { ...initialOptions, outDir: "C:/old" },
    selectedFormat: null, previewUrl: url, previewInfo: info([video]),
    refresh: vi.fn().mockResolvedValue(undefined), refreshCommand: vi.fn().mockResolvedValue(undefined),
  });
});
afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); });

describe("session presets and preview downloads", () => {
  it.each(["mp3", "720p", "custom"])("keeps %s after the options autosave", async (preset) => {
    useAppStore.getState().setOptions({ preset, customFormat: "v-720" });
    await vi.advanceTimersByTimeAsync(401);
    expect(saveSettings).toHaveBeenCalledOnce();
    expect(useAppStore.getState().options.preset).toBe(preset);
    expect(useAppStore.getState().options.customFormat).toBe("v-720");
    expect(useAppStore.getState().buildPreviewTask()).toMatchObject({ preset, customFormat: "v-720" });
    expect(useAppStore.getState().settings?.defaultPreset).toBe("mp4");
  });

  it.each([{ name: "direct HLS", formats: [] }, { name: "regular video", formats: [video] }])("honors the panel preset for $name without an explicit selection", ({ formats }) => {
    useAppStore.setState({ previewInfo: info(formats) });
    useAppStore.getState().setOptions({ preset: "720p" });
    expect(useAppStore.getState().buildPreviewTask().preset).toBe("720p");
  });

  it("uses an explicitly selected stream until the user changes the panel preset", () => {
    useAppStore.getState().selectFormat("v-1080");
    expect(useAppStore.getState().buildPreviewTask()).toMatchObject({ preset: "custom", customFormat: "v-1080+ba/v-1080/b" });
    useAppStore.getState().setOptions({ preset: "mp3" });
    expect(useAppStore.getState().selectedFormat).toBeNull();
    expect(useAppStore.getState().buildPreviewTask().preset).toBe("mp3");
  });
});

describe("settings and shared options", () => {
  it("uses the new settings directory and retains it in later autosaves", async () => {
    useAppStore.getState().setOptions({ preset: "720p" });
    const next = { ...settings(), outDir: "D:/new", outTemplate: "%(id)s.%(ext)s",
      concurrentFragments: 8, limitRate: "1M", proxy: "http://test:7890", cookiesFile: "D:/cookies.txt", cookiesBrowser: "edge" };
    await useAppStore.getState().persistSettings(next);
    expect(useAppStore.getState().buildPreviewTask()).toMatchObject({
      preset: "720p", outDir: "D:/new", outTemplate: next.outTemplate, concurrentFragments: 8,
      limitRate: "1M", proxy: next.proxy, cookiesFile: next.cookiesFile, cookiesBrowser: "edge",
    });
    useAppStore.getState().setOptions({ embedMetadata: true });
    await vi.advanceTimersByTimeAsync(401);
    expect(vi.mocked(saveSettings).mock.lastCall?.[0].outDir).toBe("D:/new");
    expect(useAppStore.getState().settings?.outDir).toBe("D:/new");
  });

  it("applies an explicitly changed default preset", async () => {
    useAppStore.getState().selectFormat("v-1080");
    await useAppStore.getState().persistSettings({ ...settings(), defaultPreset: "mp3" });
    expect(useAppStore.getState().options.preset).toBe("mp3");
    expect(useAppStore.getState().selectedFormat).toBeNull();
  });

  it("does not roll back newer session edits when an autosave completes", async () => {
    let finish!: () => void;
    vi.mocked(saveSettings).mockReturnValueOnce(new Promise<void>((resolve) => { finish = resolve; }));
    const pending = useAppStore.getState().persistSettings(settings(), "options");
    useAppStore.getState().setOptions({ preset: "mp3", outDir: "D:/newer" });
    finish();
    await pending;
    expect(useAppStore.getState().options).toMatchObject({ preset: "mp3", outDir: "D:/newer" });
  });
});
