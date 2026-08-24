import { useEffect, useState } from "react";
import { X } from "lucide-react";
import { useAppStore, useT } from "../store";
import { pickDir, pickFile, settingsDraftDirty, updateEngine, type GlobalSettings } from "../lib/ytdlp";
import { DEFAULT_PRESET } from "../presets";

const emptySettings = (): GlobalSettings => ({
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
});

export default function SettingsPage() {
  const open = useAppStore((s) => s.settingsOpen);
  const setOpen = useAppStore((s) => s.setSettingsOpen);
  const settings = useAppStore((s) => s.settings);
  const persist = useAppStore((s) => s.persistSettings);
  const t = useT();
  const lang = useAppStore((s) => s.lang);
  const setLang = useAppStore((s) => s.setLang);
  const [msg, setMsg] = useState("");
  const [draft, setDraft] = useState<GlobalSettings>(emptySettings());
  const [err, setErr] = useState("");
  const [saving, setSaving] = useState(false);
  const [updating, setUpdating] = useState(false);

  useEffect(() => {
    if (open) {
      setDraft(settings ?? emptySettings());
      setMsg("");
      setErr("");
    }
  }, [open, settings]);

  if (!open) return null;

  const field = "w-full rounded-md border border-slate-700 bg-slate-900 px-2 py-1.5 text-xs text-slate-200";
  const patch = (p: Partial<GlobalSettings>) => setDraft({ ...draft, ...p });
  const dirty = settings ? settingsDraftDirty(draft, settings) : true;

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
            {t("settings.defaultPreset")}
            <select
              className={`${field} mt-1`}
              value={draft.defaultPreset}
              onChange={(e) => patch({ defaultPreset: e.target.value })}
            >
              <option value="mp4">{t("preset.mp4")}</option>
              <option value="best">{t("preset.best")}</option>
              <option value="1080p">{t("preset.1080p")}</option>
              <option value="720p">{t("preset.720p")}</option>
              <option value="mp3">{t("preset.mp3")}</option>
              <option value="m4a">{t("preset.m4a")}</option>
            </select>
          </label>
          <label className="block text-[11px] text-slate-500">
            {t("settings.engine")}
            <div className="mt-1 flex gap-1">
              <input className={field} value={draft.enginePath ?? ""} onChange={(e) => patch({ enginePath: e.target.value || null })} />
              <button
                className="rounded-md border border-slate-700 px-2 text-xs"
                onClick={async () => {
                  const f = await pickFile();
                  if (f) patch({ enginePath: f });
                }}
              >
                …
              </button>
            </div>
          </label>
          <label className="block text-[11px] text-slate-500">
            {t("settings.ffmpeg")}
            <div className="mt-1 flex gap-1">
              <input className={field} value={draft.ffmpegPath ?? ""} onChange={(e) => patch({ ffmpegPath: e.target.value || null })} />
              <button
                className="rounded-md border border-slate-700 px-2 text-xs"
                onClick={async () => {
                  const f = await pickFile();
                  if (f) patch({ ffmpegPath: f });
                }}
              >
                …
              </button>
            </div>
          </label>
          <label className="block text-[11px] text-slate-500">
            {t("opt.dir")}
            <div className="mt-1 flex gap-1">
              <input className={field} value={draft.outDir} onChange={(e) => patch({ outDir: e.target.value })} />
              <button
                className="rounded-md border border-slate-700 px-2 text-xs"
                onClick={async () => {
                  const d = await pickDir();
                  if (d) patch({ outDir: d });
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
              max={8}
              className={`${field} mt-1`}
              value={draft.maxConcurrentTasks}
              onChange={(e) => patch({ maxConcurrentTasks: Number(e.target.value) || 2 })}
            />
          </label>
          <label className="block text-[11px] text-slate-500">
            {t("settings.lang")}
            <select className={`${field} mt-1`} value={lang} onChange={(e) => setLang(e.target.value as "zh" | "en")}>
              <option value="zh">中文</option>
              <option value="en">English</option>
            </select>
          </label>
          {err && <p className="text-xs text-red-400">{err}</p>}
          <div className="flex gap-2">
            <button
              disabled={!dirty || saving}
              className="rounded-md bg-sky-600 px-3 py-1.5 text-xs text-white hover:bg-sky-500 disabled:opacity-40"
              onClick={async () => {
                setErr("");
                setSaving(true);
                try {
                  await persist(draft);
                  setMsg(t("settings.saved"));
                } catch (e) {
                  setErr(String(e));
                } finally {
                  setSaving(false);
                }
              }}
            >
              {t("action.save")}
            </button>
            <button
              className="rounded-md border border-slate-700 px-3 py-1.5 text-xs"
              onClick={() => {
                setDraft(settings ?? emptySettings());
                setOpen(false);
              }}
            >
              {t("action.cancel")}
            </button>
          </div>
          <button
            disabled={updating}
            className="rounded-md border border-slate-700 px-3 py-1.5 text-xs text-slate-200 hover:bg-slate-800 disabled:opacity-40"
            onClick={async () => {
              setMsg("…");
              setUpdating(true);
              try {
                const r = await updateEngine();
                setMsg(r.message || `${r.oldVersion ?? "?"} → ${r.newVersion ?? "?"}`);
              } catch (e) {
                setMsg(String(e));
              } finally {
                setUpdating(false);
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
