import { useState } from "react";
import { Download, Film, ListVideo, Loader2, RefreshCw } from "lucide-react";
import { useAppStore } from "../store";
import { fmtDuration, formatOptions } from "../lib/format";
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
  return <img src={url} onError={() => setFailed(true)} alt="" className={`rounded-lg object-cover ${className}`} />;
}

function DownloadButton({ disabled, hint }: { disabled: boolean; hint: string }) {
  return (
    <button
      disabled={disabled}
      title={disabled ? hint : undefined}
      className="flex items-center gap-2 rounded-lg bg-sky-600 px-5 py-2.5 text-sm font-medium text-white transition hover:bg-sky-500 disabled:cursor-not-allowed disabled:bg-slate-700 disabled:text-slate-400"
    >
      <Download size={15} />
      下载
    </button>
  );
}

function VideoCard({ info }: { info: VideoInfo }) {
  const options = formatOptions(info.formats);
  const selectedFormat = useAppStore((s) => s.selectedFormat);
  const selectFormat = useAppStore((s) => s.selectFormat);
  const active = selectedFormat ?? options[0]?.formatId ?? null;

  const meta = [info.uploader, info.duration != null ? fmtDuration(info.duration) : null]
    .filter(Boolean)
    .join(" · ");

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-col gap-5 md:flex-row">
        <Thumb url={info.thumbnail} className="h-40 w-full shrink-0 md:w-64" />
        <div className="min-w-0 flex-1">
          <h2 className="line-clamp-2 text-base font-semibold text-slate-100">{info.title}</h2>
          {meta && <p className="mt-1 text-xs text-slate-400">{meta}</p>}
          {info.description && (
            <p className="mt-2 line-clamp-3 text-xs leading-relaxed text-slate-500">{info.description}</p>
          )}
        </div>
      </div>

      <div>
        <div className="mb-2 flex items-center justify-between">
          <span className="text-xs font-medium text-slate-400">选择清晰度</span>
          <span className="text-[11px] text-slate-600">共 {info.formats.length} 个流</span>
        </div>
        <div className="max-h-56 space-y-1 overflow-y-auto rounded-lg border border-slate-800 bg-slate-900/50 p-1.5">
          {options.length === 0 && <p className="p-3 text-xs text-slate-500">未获取到格式列表</p>}
          {options.map((o) => (
            <button
              key={o.formatId}
              onClick={() => selectFormat(o.formatId)}
              className={`flex w-full items-center justify-between rounded-md px-3 py-2 text-left text-sm transition ${
                o.formatId === active
                  ? "bg-sky-500/15 text-sky-300"
                  : "text-slate-300 hover:bg-slate-800"
              }`}
            >
              <span className="font-medium">{o.label}</span>
              <span className="text-xs text-slate-500">{o.sub.join(" · ")}</span>
            </button>
          ))}
        </div>
      </div>

      <div className="flex items-center gap-3">
        <DownloadButton disabled hint="M2 里程碑：下载队列与进度" />
        <span className="text-[11px] text-slate-600">M2 将支持：进度 / 速度 / ETA / 取消 / 断点续传</span>
      </div>
    </div>
  );
}

function PlaylistCard({ info }: { info: VideoInfo }) {
  const items = info.playlist ?? [];
  const selected = useAppStore((s) => s.selectedItems);
  const toggleItem = useAppStore((s) => s.toggleItem);
  const setAllItems = useAppStore((s) => s.setAllItems);
  const allIds = items.map((i) => i.id);
  const allSelected = allIds.length > 0 && selected.length === allIds.length;

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center gap-4">
        <ListVideo size={22} className="text-sky-400" />
        <div className="min-w-0 flex-1">
          <h2 className="truncate text-base font-semibold text-slate-100">{info.playlistTitle ?? info.title}</h2>
          <p className="text-xs text-slate-400">
            播放列表 · {info.playlistCount ?? items.length} 个视频
          </p>
        </div>
        <label className="flex cursor-pointer items-center gap-2 text-xs text-slate-300">
          <input
            type="checkbox"
            checked={allSelected}
            onChange={(e) => setAllItems(allIds, e.target.checked)}
            className="accent-sky-500"
          />
          全选（{selected.length}/{items.length}）
        </label>
      </div>

      <div className="max-h-72 space-y-1 overflow-y-auto rounded-lg border border-slate-800 bg-slate-900/50 p-1.5">
        {items.map((it, idx) => (
          <label
            key={it.id}
            className="flex cursor-pointer items-center gap-3 rounded-md px-3 py-2 text-sm hover:bg-slate-800"
          >
            <input
              type="checkbox"
              checked={selected.includes(it.id)}
              onChange={() => toggleItem(it.id)}
              className="accent-sky-500"
            />
            <span className="w-8 shrink-0 text-right text-xs text-slate-600">{idx + 1}</span>
            <span className="min-w-0 flex-1 truncate text-slate-200">{it.title}</span>
            {it.channel && <span className="hidden max-w-32 shrink-0 truncate text-xs text-slate-500 sm:block">{it.channel}</span>}
            {it.duration != null && <span className="shrink-0 text-xs text-slate-500">{fmtDuration(it.duration)}</span>}
          </label>
        ))}
      </div>

      <div className="flex items-center gap-3">
        <DownloadButton disabled hint="M5 里程碑：播放列表 / 合集下载" />
        <span className="text-[11px] text-slate-600">已选 {selected.length} 个视频</span>
      </div>
    </div>
  );
}

export default function PreviewCard() {
  const status = useAppStore((s) => s.previewStatus);
  const info = useAppStore((s) => s.previewInfo);
  const error = useAppStore((s) => s.previewError);
  const url = useAppStore((s) => s.previewUrl);
  const preview = useAppStore((s) => s.preview);

  if (status === "loading") {
    return (
      <div className="flex flex-col items-center gap-3 rounded-xl border border-slate-800 bg-slate-900/40 py-14 text-slate-400">
        <Loader2 size={22} className="animate-spin text-sky-400" />
        <p className="text-sm">正在解析 URL…（最多 20s）</p>
        <p className="max-w-md truncate text-xs text-slate-600">{url}</p>
      </div>
    );
  }

  if (status === "error") {
    return (
      <div className="flex flex-col items-start gap-3 rounded-xl border border-red-500/30 bg-red-500/5 p-5">
        <p className="text-sm font-medium text-red-300">解析失败</p>
        <p className="text-xs leading-relaxed text-slate-300">{error}</p>
        <button
          onClick={() => void preview(url)}
          className="flex items-center gap-1.5 rounded-md border border-slate-700 px-3 py-1.5 text-xs text-slate-300 hover:bg-slate-800"
        >
          <RefreshCw size={12} /> 重试
        </button>
      </div>
    );
  }

  if (status === "idle" || !info) {
    return (
      <div className="rounded-xl border border-dashed border-slate-700 bg-slate-900/30 p-10 text-center">
        <p className="text-sm text-slate-400">粘贴链接开始</p>
        <p className="mt-2 text-xs text-slate-600">
          支持 B站 / YouTube / 抖音 / TikTok / 微博 / 1000+ 站点，自动识别播放列表与合集
        </p>
      </div>
    );
  }

  return info.isPlaylist ? <PlaylistCard info={info} /> : <VideoCard info={info} />;
}
