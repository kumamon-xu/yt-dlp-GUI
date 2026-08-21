import { useEffect } from "react";
import { Download, Loader2, RotateCw, Settings } from "lucide-react";
import { useAppStore, useT } from "./store";
import type { ToolStatus } from "./lib/ytdlp";
import UrlInput from "./components/UrlInput";
import PreviewCard from "./components/PreviewCard";
import TaskTable from "./components/TaskTable";
import LogConsole from "./components/LogConsole";
import OptionsPanel from "./components/OptionsPanel";
import CommandBar from "./components/CommandBar";
import Toolbox from "./components/Toolbox";
import SettingsPage from "./components/SettingsPage";

function StatusChip({ label, status, checking }: { label: string; status: ToolStatus | null; checking: boolean }) {
  const state = checking || status === null ? "checking" : status.available ? "ok" : "err";
  const version =
    status?.version != null
      ? label === "ffmpeg"
        ? status.version.replace(/^ffmpeg version\s*/i, "")
        : label === "JS"
          ? status.version.split(" ")[0]
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
      {version && <span className="max-w-[9rem] truncate font-mono">{version}</span>}
    </div>
  );
}

export default function App() {
  const { engine, ffmpeg, jsRuntime, checking, refresh, bindTaskEvents, loadAllSettings, setSettingsOpen } =
    useAppStore();
  const t = useT();

  useEffect(() => {
    void refresh();
    void bindTaskEvents();
    void loadAllSettings();
  }, [refresh, bindTaskEvents, loadAllSettings]);

  return (
    <div className="relative flex h-screen flex-col overflow-hidden bg-[#0b0f14] text-slate-200">
      <header className="flex shrink-0 items-center justify-between border-b border-slate-800 px-4 py-3">
        <div className="flex items-center gap-2.5">
          <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-sky-500/15 text-sky-400">
            <Download size={18} />
          </div>
          <div>
            <h1 className="text-sm font-semibold leading-tight">{t("app.title")}</h1>
            <p className="text-[11px] leading-tight text-slate-500">{t("app.subtitle")}</p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <StatusChip label={t("chip.engine")} status={engine} checking={checking} />
          <StatusChip label={t("chip.ffmpeg")} status={ffmpeg} checking={checking} />
          <StatusChip label={t("chip.js")} status={jsRuntime} checking={checking} />
          <button onClick={() => void refresh()} title="refresh" className="rounded-md p-1.5 text-slate-400 hover:bg-slate-800">
            <RotateCw size={14} />
          </button>
          <button onClick={() => setSettingsOpen(true)} title={t("action.settings")} className="rounded-md p-1.5 text-slate-400 hover:bg-slate-800">
            <Settings size={14} />
          </button>
        </div>
      </header>

      <div className="shrink-0 border-b border-slate-800 px-5 py-3">
        <UrlInput />
        <CommandBar />
      </div>

      <main className="flex min-h-0 flex-1 flex-col lg:flex-row">
        <section className="min-h-0 flex-1 overflow-y-auto p-4">
          <div className="flex flex-col gap-3">
            <PreviewCard />
            <OptionsPanel />
            <Toolbox />
          </div>
        </section>
        <section className="min-h-[220px] max-h-[40vh] shrink-0 overflow-hidden border-t border-slate-800 p-4 lg:max-h-none lg:w-[380px] lg:border-l lg:border-t-0">
          <TaskTable />
        </section>
      </main>

      <LogConsole />
      <SettingsPage />
    </div>
  );
}
