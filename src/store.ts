import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { translate, type Lang } from "./i18n";
import { DEFAULT_PRESET } from "./presets";
import {
  buildCommand,
  cancelTask,
  checkEngine,
  checkFfmpeg,
  checkJsRuntime,
  getInfo,
  listTasks,
  loadSettings,
  pauseTask,
  removeTask,
  resumeTask,
  saveSettings,
  resolveDownloadPreset,
  retryTaskRequest,
  splitUrls,
  startTask,
  startTasks,
  isCurrentToken,
  acceptTaskUpdate,
  toolboxTask,
  toolError,
  type GlobalSettings,
  type NewTask,
  type TaskPayload,
  type ToolStatus,
  type VideoInfo,
} from "./lib/ytdlp";

type PreviewStatus = "idle" | "loading" | "ok" | "error";

export interface OptionsState {
  preset: string;
  customFormat: string;
  outDir: string;
  outTemplate: string;
  concurrentFragments: number;
  limitRate: string;
  proxy: string;
  cookiesBrowser: string;
  cookiesFile: string;
  writeSubs: boolean;
  embedThumbnail: boolean;
  embedMetadata: boolean;
  sponsorblock: boolean;
}

const defaultOptions = (): OptionsState => ({
  preset: DEFAULT_PRESET,
  customFormat: "",
  outDir: "",
  outTemplate: "%(title)s [%(id)s].%(ext)s",
  concurrentFragments: 4,
  limitRate: "",
  proxy: "",
  cookiesBrowser: "",
  cookiesFile: "",
  writeSubs: false,
  embedThumbnail: false,
  embedMetadata: false,
  sponsorblock: false,
});

interface AppState {
  lang: Lang;
  t: (key: string) => string;
  setLang: (lang: Lang) => void;

  engine: ToolStatus | null;
  ffmpeg: ToolStatus | null;
  jsRuntime: ToolStatus | null;
  checking: boolean;
  refresh: () => Promise<void>;

  previewStatus: PreviewStatus;
  previewUrl: string;
  previewInfo: VideoInfo | null;
  previewError: string | null;
  selectedItems: number[];
  selectedFormat: string | null;
  preview: (url: string, flat?: boolean) => Promise<void>;
  toggleItem: (index: number) => void;
  setAllItems: (indices: number[], on: boolean) => void;
  selectFormat: (id: string) => void;

  tasks: TaskPayload[];
  logs: { id: string; line: string }[];
  selectedTaskId: string | null;
  selectTask: (id: string | null) => void;
  bindTaskEvents: () => Promise<void>;
  startDownload: (task: NewTask) => Promise<void>;
  enqueueUrls: (raw: string, extra?: Partial<NewTask>) => Promise<void>;
  cancel: (id: string) => Promise<void>;
  pause: (id: string) => Promise<void>;
  resume: (id: string) => Promise<void>;
  remove: (id: string) => Promise<void>;
  retry: (id: string) => Promise<void>;

  options: OptionsState;
  setOptions: (p: Partial<OptionsState>) => void;
  settings: GlobalSettings | null;
  commandPreview: string;
  refreshCommand: () => Promise<void>;
  loadAllSettings: () => Promise<void>;
  persistSettings: (s: GlobalSettings) => Promise<void>;

  settingsOpen: boolean;
  setSettingsOpen: (v: boolean) => void;

  buildTaskFromOptions: (url: string, extra?: Partial<NewTask>) => NewTask;
}

let previewToken = 0;
let eventsBound = false;
let unlistenTaskUpdated: (() => void) | null = null;
let unlistenTaskLog: (() => void) | null = null;
let persistTimer: ReturnType<typeof setTimeout> | null = null;
let saveSeq = 0;
let commandSeq = 0;
let commandTimer: ReturnType<typeof setTimeout> | null = null;

function clearTaskEventListeners() {
  unlistenTaskUpdated?.();
  unlistenTaskLog?.();
  unlistenTaskUpdated = null;
  unlistenTaskLog = null;
  eventsBound = false;
}

function readSavedLang(): Lang {
  try {
    return localStorage.getItem("ytdlp-lang") === "en" ? "en" : "zh";
  } catch {
    return "zh";
  }
}

function mergeTask(prev: TaskPayload | undefined, next: TaskPayload): TaskPayload {
  return {
    ...next,
    request: next.request ?? prev?.request ?? { url: next.url, preset: DEFAULT_PRESET },
  };
}

export const useAppStore = create<AppState>((set, get) => ({
  lang: readSavedLang(),
  t: (key) => translate(get().lang, key),
  setLang: (lang) => {
    try {
      localStorage.setItem("ytdlp-lang", lang);
    } catch {
      /* ignore */
    }
    set({ lang });
  },

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

  preview: async (url: string, flat = true) => {
    const token = ++previewToken;
    set({ previewStatus: "loading", previewUrl: url, previewError: null, selectedFormat: null });
    try {
      const enginePath = get().settings?.enginePath ?? null;
      const info = await getInfo(url, enginePath, flat);
      if (!isCurrentToken(token, previewToken)) return;
      const items = info.playlist ?? [];
      set({
        previewStatus: "ok",
        previewInfo: info,
        selectedItems: items.map((_, i) => i + 1),
      });
    } catch (e) {
      if (!isCurrentToken(token, previewToken)) return;
      set({
        previewStatus: "error",
        previewInfo: null,
        previewError: e instanceof Error ? e.message : String(e),
      });
    }
  },

  toggleItem: (index) => {
    const cur = new Set(get().selectedItems);
    if (cur.has(index)) cur.delete(index);
    else cur.add(index);
    set({ selectedItems: [...cur] });
  },
  setAllItems: (indices, on) => set({ selectedItems: on ? indices : [] }),
  selectFormat: (id) => set({ selectedFormat: id }),

  tasks: [],
  logs: [],
  selectedTaskId: null,
  selectTask: (id) => set({ selectedTaskId: id }),

  bindTaskEvents: async () => {
    if (eventsBound) return;
    try {
      const u1 = await listen<TaskPayload>("task_updated", (e) => {
        const p = e.payload;
        set((s) => {
          const known = new Set(s.tasks.map((t) => t.id));
          if (!acceptTaskUpdate(known, p.id)) return s;
          const prev = s.tasks.find((t) => t.id === p.id);
          const row = mergeTask(prev, p);
          return { tasks: [row, ...s.tasks.filter((t) => t.id !== p.id)] };
        });
      });
      let u2: () => void;
      try {
        u2 = await listen<{ id: string; line: string }>("task_log", (e) => {
          set((s) => {
            const byId = s.logs.filter((l) => l.id === e.payload.id);
            const others = s.logs.filter((l) => l.id !== e.payload.id);
            const nextFor = [...byId, e.payload].slice(-200);
            return { logs: [...others, ...nextFor].slice(-800) };
          });
        });
      } catch (err) {
        u1();
        throw err;
      }
      unlistenTaskUpdated = u1;
      unlistenTaskLog = u2;
      eventsBound = true;
      try {
        const existing = await listTasks();
        if (existing.length) {
          set({ tasks: existing });
        }
      } catch {
        /* first launch */
      }
    } catch {
      clearTaskEventListeners();
    }
  },

  buildTaskFromOptions: (url, extra) => {
    const o = get().options;
    const s = get().settings;
    const proxy = o.proxy || s?.proxy || undefined;
    return {
      url,
      preset: resolveDownloadPreset(extra?.preset, o.preset, s?.defaultPreset || DEFAULT_PRESET),
      customFormat: o.customFormat || undefined,
      outDir: o.outDir || s?.outDir || undefined,
      outTemplate: o.outTemplate || s?.outTemplate || undefined,
      concurrentFragments: o.concurrentFragments || s?.concurrentFragments || 4,
      limitRate: o.limitRate || s?.limitRate || undefined,
      proxy,
      proxySource: proxy ? "global" : "none",
      cookiesBrowser: o.cookiesBrowser || s?.cookiesBrowser || undefined,
      cookiesFile: o.cookiesFile || s?.cookiesFile || undefined,
      writeSubs: o.writeSubs,
      embedThumbnail: o.embedThumbnail,
      embedMetadata: o.embedMetadata,
      sponsorblock: o.sponsorblock,
      noPlaylist: true,
      ...extra,
    };
  },

  startDownload: async (task) => {
    const id = String(crypto.randomUUID());
    const stub: TaskPayload = {
      id,
      url: task.url,
      title: null,
      status: "queued",
      downloaded: 0,
      total: 0,
      speed: 0,
      eta: 0,
      filePath: null,
      outputFiles: [],
      error: null,
      request: task,
      kind: task.kind,
    };
    set((s) => ({ tasks: [stub, ...s.tasks] }));
    try {
      await startTask(id, task);
    } catch {
      set((s) => ({ tasks: s.tasks.filter((t) => t.id !== id) }));
    }
  },

  enqueueUrls: async (raw, extra) => {
    const urls = splitUrls(raw);
    const o = get().options;
    const s = get().settings;
    const items = urls.map((url) => {
      const task = extra?.kind
        ? toolboxTask(url, extra.kind, {
            outDir: o.outDir || s?.outDir,
            proxy: o.proxy || s?.proxy || undefined,
            cookiesBrowser: o.cookiesBrowser || s?.cookiesBrowser || undefined,
            cookiesFile: o.cookiesFile || s?.cookiesFile || undefined,
          })
        : get().buildTaskFromOptions(url, extra);
      return { id: String(crypto.randomUUID()), task };
    });
    const stubs: TaskPayload[] = items.map(({ id, task }) => ({
      id,
      url: task.url,
      title: null,
      status: "queued",
      downloaded: 0,
      total: 0,
      speed: 0,
      eta: 0,
      filePath: null,
      outputFiles: [],
      error: null,
      request: task,
      kind: task.kind,
    }));
    set((st) => ({ tasks: [...stubs, ...st.tasks] }));
    try {
      await startTasks(items);
    } catch {
      const ids = new Set(items.map((i) => i.id));
      set((st) => ({ tasks: st.tasks.filter((t) => !ids.has(t.id)) }));
    }
  },

  cancel: (id) => cancelTask(id),
  pause: (id) => pauseTask(id),
  resume: (id) => resumeTask(id),
  remove: async (id) => {
    set((s) => ({ tasks: s.tasks.filter((t) => t.id !== id) }));
    await removeTask(id);
  },
  retry: async (id) => {
    const row = get().tasks.find((t) => t.id === id);
    if (!row) return;
    await get().startDownload(retryTaskRequest(row));
  },

  options: defaultOptions(),
  setOptions: (p) => {
    set((s) => ({ options: { ...s.options, ...p } }));
    if (persistTimer) clearTimeout(persistTimer);
    persistTimer = setTimeout(() => {
      const o = get().options;
      const cur = get().settings;
      if (!cur) return;
      void get().persistSettings({
        ...cur,
        outDir: o.outDir,
        outTemplate: o.outTemplate || cur.outTemplate,
        concurrentFragments: o.concurrentFragments || cur.concurrentFragments,
        limitRate: o.limitRate ? o.limitRate : null,
        proxy: o.proxy ? o.proxy : null,
        cookiesBrowser: o.cookiesBrowser ? o.cookiesBrowser : null,
        cookiesFile: o.cookiesFile ? o.cookiesFile : null,
      });
    }, 400);
  },
  settings: null,
  commandPreview: "",
  refreshCommand: async () => {
    if (commandTimer) clearTimeout(commandTimer);
    commandTimer = setTimeout(() => {
      const token = ++commandSeq;
      const url = get().previewUrl.trim();
      if (!/^https?:\/\//i.test(url)) {
        set({ commandPreview: "" });
        return;
      }
      void buildCommand(get().buildTaskFromOptions(url)).then(
        (cmd) => {
          if (!isCurrentToken(token, commandSeq)) return;
          set({ commandPreview: cmd });
        },
        () => {
          if (!isCurrentToken(token, commandSeq)) return;
          set({ commandPreview: "" });
        },
      );
    }, 200);
  },
  loadAllSettings: async () => {
    try {
      const settings = await loadSettings();
      set({
        settings,
        options: {
          ...get().options,
          preset: settings.defaultPreset || DEFAULT_PRESET,
          outDir: settings.outDir || get().options.outDir,
          outTemplate: settings.outTemplate || get().options.outTemplate,
          concurrentFragments: settings.concurrentFragments || 4,
          limitRate: settings.limitRate || "",
          proxy: settings.proxy || "",
          cookiesBrowser: settings.cookiesBrowser || "",
          cookiesFile: settings.cookiesFile || "",
        },
      });
    } catch {
      /* ignore */
    }
  },
  persistSettings: async (settings) => {
    const token = ++saveSeq;
    await saveSettings(settings);
    if (token !== saveSeq) return;
    set({
      settings,
      options: { ...get().options, preset: settings.defaultPreset || get().options.preset },
    });
  },

  settingsOpen: false,
  setSettingsOpen: (v) => set({ settingsOpen: v }),
}));

/** 订阅语言，切换中/英时组件会重渲染 */
export function useT(): (key: string) => string {
  const lang = useAppStore((s) => s.lang);
  return (key: string) => translate(lang, key);
}
