import { useEffect } from "react";
import { Download, Loader2, RotateCw } from "lucide-react";
import { useAppStore } from "./store";
import type { ToolStatus } from "./lib/ytdlp";

function StatusChip({ label, status, checking }: { label: string; status: ToolStatus | null; checking: boolean }) {
  const state = checking || status === null ? "checking" : status.available ? "ok" : "err";
  const version =
    status?.version != null
      ? label === "ffmpeg"
        ? status.version.replace(/^ffmpeg version\s*/i, "")
        : status.version
      : null;
  const tip = status?.error ?? status?.path ?? "";
  return (
    <div
      title={tip}
      className={`flex items-center gap-1.5 rounded-full border px-3 py-1 text-xs ${
        state === "ok"
          ? "border-emerald-500/40 bg-emerald-500/10 text-emerald-300"
          : state === "err"
            ? "border-red-500/40 bg-red-500/10 text-red-300"
            : "border-slate-600/40 bg-slate-700/20 text-slate-400"
      }`}
    >
      {state === "checking" ? (
        <Loader2 size={12} className="animate-spin" />
      ) : (
        <span className={`h-2 w-2 rounded-full ${state === "ok" ? "bg-emerald-400" : "bg-red-400"}`} />
      )}
      <span>{label}</span>
      {version && <span className="font-mono">{version}</span>}
    </div>
  );
}

export default function App() {
  const { engine, ffmpeg, checking, refresh } = useAppStore();

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return (
    <div className="flex h-screen flex-col bg-[#0b0f14] text-slate-200">
      <header className="flex items-center justify-between border-b border-slate-800 px-4 py-3">
        <div className="flex items-center gap-2.5">
          <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-sky-500/15 text-sky-400">
            <Download size={18} />
          </div>
          <div>
            <h1 className="text-sm font-semibold leading-tight">yt-dlp GUI</h1>
            <p className="text-[11px] leading-tight text-slate-500">多平台视频解析下载器</p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <StatusChip label="引擎" status={engine} checking={checking} />
          <StatusChip label="ffmpeg" status={ffmpeg} checking={checking} />
          <button
            onClick={() => void refresh()}
            title="重新检测"
            className="rounded-md p-1.5 text-slate-400 hover:bg-slate-800 hover:text-slate-200"
          >
            <RotateCw size={14} />
          </button>
        </div>
      </header>

      <main className="flex flex-1 flex-col items-center justify-center gap-4 p-8">
        <div className="max-w-md rounded-xl border border-dashed border-slate-700 bg-slate-900/40 p-8 text-center">
          <p className="text-sm text-slate-400">M0 脚手架就绪 ✅</p>
          <p className="mt-2 text-xs leading-relaxed text-slate-500">
            顶部状态栏已检测 yt-dlp 引擎与 ffmpeg。
            <br />
            下一步（M1）：粘贴 URL 即时预览 —— 标题 / 封面 / 时长 / 清晰度选择。
          </p>
        </div>
        {engine?.path && (
          <p className="max-w-lg truncate text-[11px] text-slate-600" title={engine.path}>
            引擎路径：{engine.path}
          </p>
        )}
      </main>
    </div>
  );
}
