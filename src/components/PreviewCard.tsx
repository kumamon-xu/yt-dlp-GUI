import { useState } from "react";
import { Download, Film, ListVideo, Loader2, RefreshCw } from "lucide-react";
import { useAppStore, useT } from "../store";
import { customFormatFromSelection, fmtDuration, formatOptions } from "../lib/format";
import { compressPlaylistItems } from "../lib/ytdlp";
import type { VideoInfo } from "../lib/ytdlp";

function Thumb({ url, className }: { url: string | null; className: string }) {
  const [failed, setFailed] = useState(false);
  if (!url || failed) {
    return (
      <div className={`flex items-center justify-center rounded-lg bg-slate-800 text-slate-600 ${className}`}>
        <Film size={28} />
      </div>
    );
  }
  return (
    <img
      src={url}
      referrerPolicy="no-referrer"
      onError={() => setFailed(true)}
      alt=""
      className={`rounded-lg object-cover ${className}`}
    />
  );
}

function VideoCard({ info }: { info: VideoInfo }) {
  const options = formatOptions(info.formats);
  const selectedFormat = useAppStore((s) => s.selectedFormat);
  const selectFormat = useAppStore((s) => s.selectFormat);
  const startDownload = useAppStore((s) => s.startDownload);
  const buildTask = useAppStore((s) => s.buildTaskFromOptions);
  const previewUrl = useAppStore((s) => s.previewUrl);
  const t = useT();
  const active = selectedFormat ?? options[0]?.formatId ?? null;
  const activeOption = options.find((o) => o.formatId === active) ?? null;
  const meta = [info.uploader, info.duration != null ? fmtDuration(info.duration) : null].filter(Boolean).join(" · ");

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-4 md:flex-row">
        <Thumb url={info.thumbnail} className="h-36 w-full shrink-0 md:w-56" />
        <div className="min-w-0 flex-1">
          <h2 className="line-clamp-2 text-base font-semibold text-slate-100">{info.title}</h2>
          {meta && <p className="mt-1 text-xs text-slate-400">{meta}</p>}
          {info.description && <p className="mt-2 line-clamp-3 text-xs leading-relaxed text-slate-500">{info.description}</p>}
        </div>
      </div>
      <div>
        <div className="mb-2 flex items-center justify-between">
          <span className="text-xs font-medium text-slate-400">{t("preview.pickFormat")}</span>
          <span className="text-[11px] text-slate-600">
            {info.formats.length} {t("preview.streams")}
          </span>
        </div>
        <div className="max-h-48 space-y-1 overflow-y-auto rounded-lg border border-slate-800 bg-slate-900/50 p-1.5">
          {options.length === 0 && <p className="p-3 text-xs text-slate-500">—</p>}
          {options.map((o) => (
            <button
              key={o.formatId}
              onClick={() => selectFormat(o.formatId)}
              className={`flex w-full items-center justify-between rounded-md px-3 py-2 text-left text-sm transition ${
                o.formatId === active ? "bg-sky-500/15 text-sky-300" : "text-slate-300 hover:bg-slate-800"
              }`}
            >
              <span className="font-medium">{o.label}</span>
              <span className="text-xs text-slate-500">{o.sub.join(" · ")}</span>
            </button>
          ))}
        </div>
      </div>
      <button
        onClick={() => {
          const sel = customFormatFromSelection(activeOption);
          void startDownload(
            buildTask(previewUrl, {
              ...sel,
              noPlaylist: true,
            }),
          );
        }}
        className="flex items-center justify-center gap-2 rounded-lg bg-sky-600 px-5 py-2.5 text-sm font-medium text-white hover:bg-sky-500"
      >
        <Download size={15} />
        {t("action.download")}
      </button>
    </div>
  );
}

function PlaylistCard({ info }: { info: VideoInfo }) {
  const [shown, setShown] = useState(100);
  const items = (info.playlist ?? []).slice(0, shown);
  const selected = useAppStore((s) => s.selectedItems);
  const toggleItem = useAppStore((s) => s.toggleItem);
  const setAllItems = useAppStore((s) => s.setAllItems);
  const startDownload = useAppStore((s) => s.startDownload);
  const buildTask = useAppStore((s) => s.buildTaskFromOptions);
  const previewUrl = useAppStore((s) => s.previewUrl);
  const preview = useAppStore((s) => s.preview);
  const t = useT();
  const all = info.playlist ?? [];
  const allIdx = all.map((_, i) => i + 1);
  const selectedSet = new Set(selected);
  const allSelected = allIdx.length > 0 && selected.length === allIdx.length;
  const playlistItems = selected.length === all.length ? undefined : compressPlaylistItems(selected);

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center gap-4">
        <ListVideo size={22} className="text-sky-400" />
        <div className="min-w-0 flex-1">
          <h2 className="truncate text-base font-semibold text-slate-100">{info.playlistTitle ?? info.title}</h2>
          <p className="text-xs text-slate-400">
            {t("preview.playlist")} · {info.playlistCount ?? items.length}
          </p>
        </div>
        <label className="flex cursor-pointer items-center gap-2 text-xs text-slate-300">
          <input type="checkbox" checked={allSelected} onChange={(e) => setAllItems(allIdx, e.target.checked)} className="accent-sky-500" />
          {t("preview.selectAll")}（{selected.length}/{all.length}）
        </label>
      </div>
      <div className="max-h-56 space-y-1 overflow-y-auto rounded-lg border border-slate-800 bg-slate-900/50 p-1.5">
        {items.map((it, idx) => (
          <div key={`${idx}:${it.id}`} className="flex items-center gap-3 rounded-md px-3 py-2 text-sm hover:bg-slate-800">
            <input type="checkbox" checked={selectedSet.has(idx + 1)} onChange={() => toggleItem(idx + 1)} className="accent-sky-500" />
            <span className="w-8 shrink-0 text-right text-xs text-slate-600">{idx + 1}</span>
            <button
              type="button"
              className="min-w-0 flex-1 truncate text-left text-slate-200 hover:text-sky-300"
              title={t("preview.openItem")}
              onClick={() => {
                if (it.webpageUrl) void preview(it.webpageUrl, false);
              }}
            >
              {it.title}
            </button>
            {it.duration != null && <span className="shrink-0 text-xs text-slate-500">{fmtDuration(it.duration)}</span>}
          </div>
        ))}
      </div>
      {all.length > shown && (
        <button className="text-xs text-sky-400" onClick={() => setShown((n) => n + 100)}>
          {t("preview.loadMore")} ({shown}/{all.length})
        </button>
      )}
      <button
        disabled={selected.length === 0}
        onClick={() =>
          void startDownload(
            buildTask(previewUrl, {
              noPlaylist: false,
              playlistItems: selected.length === all.length ? undefined : playlistItems,
            }),
          )
        }
        className="flex items-center justify-center gap-2 rounded-lg bg-sky-600 px-5 py-2.5 text-sm font-medium text-white hover:bg-sky-500 disabled:opacity-40"
      >
        <Download size={15} />
        {t("action.download")} · {selected.length}
      </button>
    </div>
  );
}

export default function PreviewCard() {
  const status = useAppStore((s) => s.previewStatus);
  const info = useAppStore((s) => s.previewInfo);
  const error = useAppStore((s) => s.previewError);
  const url = useAppStore((s) => s.previewUrl);
  const preview = useAppStore((s) => s.preview);
  const t = useT();

  if (status === "loading") {
    return (
      <div className="flex flex-col items-center gap-3 rounded-xl border border-slate-800 bg-slate-900/40 py-10 text-slate-400">
        <Loader2 size={22} className="animate-spin text-sky-400" />
        <p className="text-sm">{t("preview.loading")}</p>
        <p className="max-w-md truncate text-xs text-slate-600">{url}</p>
      </div>
    );
  }

  if (status === "error") {
    return (
      <div className="flex flex-col items-start gap-3 rounded-xl border border-red-500/30 bg-red-500/5 p-5">
        <p className="text-sm font-medium text-red-300">{t("preview.fail")}</p>
        <p className="text-xs leading-relaxed text-slate-300">{error}</p>
        <button
          onClick={() => void preview(url)}
          className="flex items-center gap-1.5 rounded-md border border-slate-700 px-3 py-1.5 text-xs text-slate-300 hover:bg-slate-800"
        >
          <RefreshCw size={12} /> {t("preview.retry")}
        </button>
      </div>
    );
  }

  if (status === "idle" || !info) {
    return (
      <div className="rounded-xl border border-dashed border-slate-700 bg-slate-900/30 p-8 text-center">
        <p className="text-sm text-slate-400">{t("preview.idle")}</p>
      </div>
    );
  }

  return info.isPlaylist ? <PlaylistCard info={info} /> : <VideoCard info={info} />;
}
