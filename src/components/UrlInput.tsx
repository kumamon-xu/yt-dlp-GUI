import { useState } from "react";
import { Link2, Loader2, Search, Zap } from "lucide-react";
import { useAppStore, useT } from "../store";
import { splitUrls } from "../lib/ytdlp";
import { DEFAULT_PRESET } from "../presets";

export default function UrlInput() {
  const [value, setValue] = useState("");
  const preview = useAppStore((s) => s.preview);
  const status = useAppStore((s) => s.previewStatus);
  const enqueueUrls = useAppStore((s) => s.enqueueUrls);
  const defaultPreset = useAppStore((s) => s.settings?.defaultPreset ?? DEFAULT_PRESET);
  const t = useT();

  const urls = splitUrls(value);
  const first = urls[0] ?? "";

  const submitParse = () => {
    if (!first) return;
    void preview(first);
  };

  const quick = () => {
    if (!urls.length) return;
    void enqueueUrls(value, {
      preset: defaultPreset,
      noPlaylist: true,
    });
  };

  return (
    <div className="flex items-start gap-2">
      <div className="relative flex-1">
        <Link2 size={15} className="pointer-events-none absolute left-3 top-3 text-slate-500" />
        <textarea
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              submitParse();
            }
          }}
          rows={2}
          placeholder={t("url.placeholder")}
          className="w-full resize-none rounded-lg border border-slate-700 bg-slate-900 py-2.5 pl-9 pr-3 text-sm text-slate-200 placeholder:text-slate-600 focus:border-sky-500 focus:outline-none focus:ring-1 focus:ring-sky-500/40"
        />
      </div>
      <button
        onClick={submitParse}
        disabled={!first || status === "loading"}
        className="flex items-center gap-1.5 rounded-lg bg-sky-600 px-4 py-2.5 text-sm font-medium text-white transition hover:bg-sky-500 disabled:cursor-not-allowed disabled:opacity-40"
      >
        {status === "loading" ? <Loader2 size={14} className="animate-spin" /> : <Search size={14} />}
        {t("action.parse")}
      </button>
      <button
        onClick={quick}
        disabled={!urls.length}
        title={t("action.quick")}
        className="flex items-center gap-1.5 rounded-lg border border-slate-600 bg-slate-800 px-3 py-2.5 text-sm text-slate-200 hover:bg-slate-700 disabled:opacity-40"
      >
        <Zap size={14} />
        {t("action.quick")}
      </button>
    </div>
  );
}
