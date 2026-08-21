import { useState } from "react";
import { X } from "lucide-react";
import { useAppStore, useT } from "../store";
import { pickDir, pickFile, updateEngine, type GlobalSettings } from "../lib/ytdlp";
import { DEFAULT_PRESET } from "../presets";

export default function SettingsPage() {
  const open = useAppStore((s) => s.settingsOpen);
  const setOpen = useAppStore((s) => s.setSettingsOpen);
  const settings = useAppStore((s) => s.settings);
  const persist = useAppStore((s) => s.persistSettings);
  const t = useT();
  const lang = useAppStore((s) => s.lang);
  const setLang = useAppStore((s) => s.setLang);
  const [msg, setMsg] = useState("");

  if (!open) return null;

  const s: GlobalSettings = settings ?? {
    defaultPreset: DEFAULT_PRESET,
    outDir: "",
    outTemplate: "%(title)s [%(id)s].%(ext)s",
    concurrentFragments: 4,
    maxConcurrentTasks: 2,
    limitRate: null,
    cookiesBrowser: null,
    cookiesFile: null,
    proxy: null,
    enginePath: null,
    ffmpegPath: null,
    mergeFormat: "mp4",
  };

  const patch = (p: Partial<GlobalSettings>) => persist({ ...s, ...p });
  const field = "w-full rounded-md border border-slate-700 bg-slate-900 px-2 py-1.5 text-xs text-slate-200";

  return (
    <div className="absolute inset-0 z-20 flex items-center justify-center bg-black/60 p-6">
      <div className="max-h-full w-full max-w-lg overflow-y-auto rounded-xl border border-slate-700 bg-[#111820] p-4">
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-sm font-semibold">{t("settings.title")}</h2>
          <button onClick={() => setOpen(false)} className="rounded p-1 hover:bg-slate-800">
            <X size={16} />
          </button>
        </div>
        <div className="space-y-3">
          <label className="block text-[11px] text-slate-500">
            {t("settings.engine")}
            <div className="mt-1 flex gap-1">
              <input className={field} value={s.enginePath ?? ""} onChange={(e) => void patch({ enginePath: e.target.value || null })} />
              <button
                className="rounded-md border border-slate-700 px-2 text-xs"
                onClick={async () => {
                  const f = await pickFile();
                  if (f) void patch({ enginePath: f });
                }}
              >
                …
              </button>
            </div>
          </label>
          <label className="block text-[11px] text-slate-500">
            {t("settings.ffmpeg")}
            <div className="mt-1 flex gap-1">
              <input className={field} value={s.ffmpegPath ?? ""} onChange={(e) => void patch({ ffmpegPath: e.target.value || null })} />
              <button
                className="rounded-md border border-slate-700 px-2 text-xs"
                onClick={async () => {
                  const f = await pickFile();
                  if (f) void patch({ ffmpegPath: f });
                }}
              >
                …
              </button>
            </div>
          </label>
          <label className="block text-[11px] text-slate-500">
            {t("opt.dir")}
            <div className="mt-1 flex gap-1">
              <input className={field} value={s.outDir} onChange={(e) => void patch({ outDir: e.target.value })} />
              <button
                className="rounded-md border border-slate-700 px-2 text-xs"
                onClick={async () => {
                  const d = await pickDir();
                  if (d) void patch({ outDir: d });
                }}
              >
                …
              </button>
            </div>
          </label>
          <label className="block text-[11px] text-slate-500">
            {t("settings.max")}
            <input
              type="number"
              min={1}
              className={`${field} mt-1`}
              value={s.maxConcurrentTasks}
              onChange={(e) => void patch({ maxConcurrentTasks: Number(e.target.value) || 2 })}
            />
          </label>
          <label className="block text-[11px] text-slate-500">
            {t("settings.lang")}
            <select className={`${field} mt-1`} value={lang} onChange={(e) => setLang(e.target.value as "zh" | "en")}>
              <option value="zh">中文</option>
              <option value="en">English</option>
            </select>
          </label>
          <button
            className="rounded-md bg-sky-600 px-3 py-1.5 text-xs text-white hover:bg-sky-500"
            onClick={async () => {
              setMsg("…");
              try {
                setMsg(await updateEngine());
              } catch (e) {
                setMsg(String(e));
              }
            }}
          >
            {t("settings.update")}
          </button>
          {msg && <pre className="max-h-32 overflow-auto whitespace-pre-wrap text-[11px] text-slate-400">{msg}</pre>}
        </div>
      </div>
    </div>
  );
}
