import { useEffect, useRef, useState } from "react";
import { Plus, Search, Trash2, X } from "lucide-react";
import { analyzeHls, cancelHlsAnalysis, normalizeHlsMirrors, type GlobalSettings, type HlsAnalysis, type HlsSettings } from "../lib/ytdlp";
import { useAppStore, useT } from "../store";

export default function HlsSettingsDialog({ settings, onClose, onSaved }: {
  settings: GlobalSettings;
  onClose: () => void;
  onSaved: (value: HlsSettings) => void;
}) {
  const t = useT();
  const persist = useAppStore((s) => s.persistHlsSettings);
  const [preflight, setPreflight] = useState(settings.hlsPreflight ?? true);
  const [automatic, setAutomatic] = useState(settings.hlsAutoDiscovery ?? true);
  const [rules, setRules] = useState(settings.hlsMirrors ?? []);
  const [filter, setFilter] = useState("");
  const [url, setUrl] = useState("");
  const [report, setReport] = useState<HlsAnalysis | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [saving, setSaving] = useState(false);
  const session = useRef<string | null>(null);
  const dialog = useRef<HTMLDivElement>(null);
  const close = useRef(onClose);
  close.current = onClose;

  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    dialog.current?.focus();
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") { e.preventDefault(); close.current(); }
      if (e.key !== "Tab") return;
      const controls = Array.from(dialog.current?.querySelectorAll<HTMLElement>("button:not(:disabled),input:not(:disabled),[tabindex='0']") ?? []);
      const first = controls[0], last = controls[controls.length - 1];
      if (e.shiftKey && (document.activeElement === first || document.activeElement === dialog.current)) { e.preventDefault(); last?.focus(); }
      else if (!e.shiftKey && (document.activeElement === last || document.activeElement === dialog.current)) { e.preventDefault(); first?.focus(); }
    };
    document.addEventListener("keydown", handleKey);
    return () => {
      document.removeEventListener("keydown", handleKey);
      if (session.current) void cancelHlsAnalysis(session.current).catch(() => {});
      session.current = null;
      previous?.focus();
    };
  }, []);

  const field = "w-full min-w-0 rounded-md border border-slate-700 bg-slate-900 px-2 py-1.5 text-xs text-slate-200";
  const values = (): HlsSettings => ({ hlsPreflight: preflight, hlsAutoDiscovery: automatic, hlsMirrors: normalizeHlsMirrors(rules) });
  const filtered = rules.map((rule, index) => ({ rule, index })).filter(({ rule }) =>
    `${rule.fromOrigin} ${rule.toOrigin} ${rule.pathPrefix}`.toLowerCase().includes(filter.toLowerCase()));

  return <div className="absolute inset-0 z-30 flex items-center justify-center bg-black/70 p-5">
    <div ref={dialog} role="dialog" aria-modal="true" aria-labelledby="hls-settings-title" tabIndex={-1}
      className="flex max-h-full w-full max-w-3xl flex-col rounded-xl border border-slate-700 bg-[#111820] outline-none">
      <div className="flex shrink-0 items-center justify-between border-b border-slate-800 px-5 py-3">
        <h2 id="hls-settings-title" className="text-sm font-semibold">{t("settings.hlsTitle")}</h2>
        <button aria-label={t("action.close")} onClick={onClose} className="rounded p-1 hover:bg-slate-800"><X size={16} /></button>
      </div>
      <div className="min-h-0 space-y-4 overflow-y-auto p-5">
        <div className="space-y-2">
          <label className="flex items-center gap-2 text-xs"><input type="checkbox" disabled={busy || saving} checked={preflight} onChange={(e) => { setPreflight(e.target.checked); setReport(null); }} />{t("settings.hlsPreflight")}</label>
          <label className="flex items-center gap-2 text-xs"><input type="checkbox" disabled={busy || saving} checked={automatic} onChange={(e) => { setAutomatic(e.target.checked); setReport(null); }} />{t("settings.hlsAutomatic")}</label>
          <p className="text-[11px] leading-relaxed text-slate-400">{t("settings.hlsAutomaticHint")}</p>
          {!preflight && <p className="text-[11px] text-amber-300">{t("settings.hlsDisabledHint")}</p>}
        </div>
        <div className="space-y-2">
          <div className="flex items-center justify-between gap-2">
            <h3 className="text-xs">{t("settings.hlsMirrors")} <span className="text-slate-500">{rules.length}/32</span></h3>
            <button disabled={rules.length >= 32 || busy || saving} className="flex items-center gap-1 rounded border border-slate-700 px-2 py-1 text-xs disabled:opacity-40"
              onClick={() => { setFilter(""); setReport(null); setRules([...rules, { fromOrigin: "", toOrigin: "", pathPrefix: "" }]); }}><Plus size={13} />{t("settings.hlsAdd")}</button>
          </div>
          <p className="text-[11px] leading-relaxed text-slate-400">{t("settings.hlsMirrorsHint")}</p>
          <input aria-label={t("settings.hlsFilter")} placeholder={t("settings.hlsFilter")} className={field} value={filter} onChange={(e) => setFilter(e.target.value)} />
          <div className="max-h-60 overflow-auto rounded-md border border-slate-700">
            <table className="w-full min-w-[620px] table-fixed text-left text-[11px]">
              <thead className="sticky top-0 bg-slate-900 text-slate-400"><tr>
                <th className="w-[34%] p-2">{t("settings.hlsFrom")}</th><th className="w-[34%] p-2">{t("settings.hlsTo")}</th><th className="p-2">{t("settings.hlsPrefix")}</th><th className="w-9"><span className="sr-only">{t("action.remove")}</span></th>
              </tr></thead>
              <tbody>{filtered.map(({ rule, index }) => <tr key={index} className="border-t border-slate-800">
                {(["fromOrigin", "toOrigin", "pathPrefix"] as const).map((key) => <td key={key} className="p-1.5">
                  <input aria-label={`${t(key === "fromOrigin" ? "settings.hlsFrom" : key === "toOrigin" ? "settings.hlsTo" : "settings.hlsPrefix")} ${index + 1}`}
                    disabled={busy || saving} className={field} value={rule[key]} placeholder={key === "fromOrigin" ? "https://cdn.example.com" : key === "toOrigin" ? "https://backup.example.com" : "/"}
                    onChange={(e) => { setReport(null); setRules(rules.map((r, i) => i === index ? { ...r, [key]: e.target.value } : r)); }} />
                </td>)}
                <td className="p-1"><button disabled={busy || saving} aria-label={`${t("action.remove")} ${index + 1}`} onClick={() => { setReport(null); setRules(rules.filter((_, i) => i !== index)); }} className="rounded p-1 text-red-400 hover:bg-slate-800"><Trash2 size={14} /></button></td>
              </tr>)}</tbody>
            </table>
            {filtered.length === 0 && <p className="p-3 text-xs text-slate-500">{t(rules.length ? "settings.hlsNoMatches" : "settings.hlsEmpty")}</p>}
          </div>
          <p className="text-[11px] text-slate-500">{t("settings.hlsPrefixHint")}</p>
        </div>
        <div className="space-y-2 rounded-md border border-slate-700 p-3">
          <h3 className="text-xs">{t("settings.hlsAnalyze")}</h3>
          <p className="text-[11px] leading-relaxed text-slate-400">{t("settings.hlsAnalyzeHint")}</p>
          <div className="flex gap-2">
            <input disabled={busy || saving} className={field} aria-label={t("settings.hlsAnalyzeUrl")} placeholder={t("settings.hlsAnalyzeUrl")} value={url} onChange={(e) => { setUrl(e.target.value); setReport(null); }} />
            <button disabled={busy || saving || !/^https?:\/\//i.test(url.trim())} className="flex shrink-0 items-center gap-1 rounded bg-sky-600 px-3 text-xs disabled:opacity-40"
              onClick={async () => {
                const id = crypto.randomUUID(); session.current = id;
                setBusy(true); setError(""); setReport(null);
                try { const result = await analyzeHls(id, url.trim(), { ...settings, ...values() }); if (session.current === id) setReport(result); }
                catch (e) { if (session.current === id) setError(String(e)); }
                finally { if (session.current === id) { session.current = null; setBusy(false); } }
              }}><Search size={13} />{t(busy ? "settings.hlsAnalyzing" : "action.analyze")}</button>
          </div>
          {busy && <button className="text-xs text-slate-400" onClick={() => {
            const id = session.current; session.current = null; setBusy(false);
            if (id) void cancelHlsAnalysis(id).catch(() => {});
          }}>{t("action.cancel")}</button>}
          {report && <div role="status" className="max-h-36 space-y-1 overflow-y-auto text-[11px] text-emerald-300">
            {report.messages.map((message, i) => <p key={i}>{message}</p>)}
            <p className="text-slate-400">{t("settings.hlsDomains")}: {report.domains.join(" · ")}</p>
          </div>}
        </div>
        {error && <p role="alert" className="text-xs text-red-400">{error}</p>}
      </div>
      <div className="flex shrink-0 justify-end gap-2 border-t border-slate-800 px-5 py-3">
        <button onClick={onClose} className="rounded border border-slate-700 px-3 py-1.5 text-xs">{t("action.cancel")}</button>
        <button disabled={saving || busy} className="rounded bg-sky-600 px-3 py-1.5 text-xs disabled:opacity-40" onClick={async () => {
          setSaving(true); setError("");
          try { const value = values(); await persist(value); onSaved(value); onClose(); }
          catch (e) { setError(String(e)); }
          finally { setSaving(false); }
        }}>{t("action.save")}</button>
      </div>
    </div>
  </div>;
}
