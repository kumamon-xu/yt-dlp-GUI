import { useAppStore, useT } from "../store";
import { PRESET_IDS } from "../presets";
import { pickDir, pickFile } from "../lib/ytdlp";

export default function OptionsPanel() {
  const o = useAppStore((s) => s.options);
  const set = useAppStore((s) => s.setOptions);
  const t = useT();
  const ffmpeg = useAppStore((s) => s.ffmpeg);
  const noFf = ffmpeg != null && !ffmpeg.available;

  const field = "w-full rounded-md border border-slate-700 bg-slate-900 px-2 py-1.5 text-xs text-slate-200";

  return (
    <div className="rounded-xl border border-slate-800 bg-slate-900/40 p-3">
      <h3 className="mb-2 text-xs font-semibold text-slate-300">{t("opt.title")}</h3>
      <div className="grid grid-cols-2 gap-2">
        <label className="col-span-1 text-[11px] text-slate-500">
          {t("opt.preset")}
          <select
            className={`${field} mt-1`}
            value={o.preset}
            onChange={(e) => set({ preset: e.target.value })}
          >
            {PRESET_IDS.map((id) => (
              <option key={id} value={id} disabled={noFf && (id === "mp3" || id === "m4a")}>
                {t(`preset.${id}`)}
              </option>
            ))}
          </select>
        </label>
        <label className="col-span-1 text-[11px] text-slate-500">
          {t("opt.customF")}
          <input
            className={`${field} mt-1 font-mono`}
            value={o.customFormat}
            onChange={(e) => set({ customFormat: e.target.value, preset: e.target.value ? "custom" : o.preset })}
          />
        </label>
        <label className="col-span-2 text-[11px] text-slate-500">
          {t("opt.dir")}
          <div className="mt-1 flex gap-1">
            <input className={field} value={o.outDir} onChange={(e) => set({ outDir: e.target.value })} />
            <button
              className="rounded-md border border-slate-700 px-2 text-xs text-slate-300 hover:bg-slate-800"
              onClick={async () => {
                const d = await pickDir();
                if (d) set({ outDir: d });
              }}
            >
              …
            </button>
          </div>
        </label>
        <label className="col-span-2 text-[11px] text-slate-500">
          {t("opt.template")}
          <input className={`${field} mt-1 font-mono`} value={o.outTemplate} onChange={(e) => set({ outTemplate: e.target.value })} />
        </label>
        <label className="text-[11px] text-slate-500">
          {t("opt.fragments")}
          <input
            type="number"
            min={1}
            className={`${field} mt-1`}
            value={o.concurrentFragments}
            onChange={(e) => set({ concurrentFragments: Number(e.target.value) || 4 })}
          />
        </label>
        <label className="text-[11px] text-slate-500">
          {t("opt.rate")}
          <input className={`${field} mt-1`} placeholder="1M" value={o.limitRate} onChange={(e) => set({ limitRate: e.target.value })} />
        </label>
        <label className="col-span-2 text-[11px] text-slate-500">
          {t("opt.proxy")}
          <input className={`${field} mt-1`} placeholder="http://127.0.0.1:7890" value={o.proxy} onChange={(e) => set({ proxy: e.target.value })} />
        </label>
        <label className="text-[11px] text-slate-500">
          {t("opt.cookiesBrowser")}
          <select className={`${field} mt-1`} value={o.cookiesBrowser} onChange={(e) => set({ cookiesBrowser: e.target.value })}>
            <option value="">{t("opt.none")}</option>
            <option value="edge">Edge</option>
            <option value="chrome">Chrome</option>
            <option value="firefox">Firefox</option>
            <option value="brave">Brave</option>
          </select>
        </label>
        <label className="text-[11px] text-slate-500">
          {t("opt.cookiesFile")}
          <div className="mt-1 flex gap-1">
            <input className={field} value={o.cookiesFile} onChange={(e) => set({ cookiesFile: e.target.value })} />
            <button
              className="rounded-md border border-slate-700 px-2 text-xs text-slate-300 hover:bg-slate-800"
              onClick={async () => {
                const f = await pickFile();
                if (f) set({ cookiesFile: f });
              }}
            >
              …
            </button>
          </div>
        </label>
        <p className="col-span-2 text-[10px] text-amber-500/80">{t("opt.cookiesHint")}</p>
        <label className="flex items-center gap-2 text-[11px] text-slate-300">
          <input type="checkbox" checked={o.writeSubs} onChange={(e) => set({ writeSubs: e.target.checked })} />
          {t("opt.subs")}
        </label>
        <label className="flex items-center gap-2 text-[11px] text-slate-300">
          <input type="checkbox" checked={o.embedThumbnail} onChange={(e) => set({ embedThumbnail: e.target.checked })} />
          {t("opt.embedThumb")}
        </label>
        <label className="flex items-center gap-2 text-[11px] text-slate-300">
          <input type="checkbox" checked={o.embedMetadata} onChange={(e) => set({ embedMetadata: e.target.checked })} />
          {t("opt.embedMeta")}
        </label>
        <label className="flex items-center gap-2 text-[11px] text-slate-300">
          <input type="checkbox" checked={o.sponsorblock} onChange={(e) => set({ sponsorblock: e.target.checked })} />
          {t("opt.sponsor")}
        </label>
      </div>
    </div>
  );
}
