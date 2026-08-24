import { Download, FolderOpen, Pause, Play, RotateCcw, Trash2, X } from "lucide-react";
import { canCancelStatus, canRemoveStatus, openFolder } from "../lib/ytdlp";
import { fmtEta, fmtSize, fmtSpeed } from "../lib/format";
import { useAppStore, useT } from "../store";

function TaskRow({ id }: { id: string }) {
  const task = useAppStore((s) => s.tasks.find((t) => t.id === id) ?? null);
  const t = useT();
  const cancel = useAppStore((s) => s.cancel);
  const pause = useAppStore((s) => s.pause);
  const resume = useAppStore((s) => s.resume);
  const remove = useAppStore((s) => s.remove);
  const retry = useAppStore((s) => s.retry);
  if (!task) return null;

  const stKey = `status.${task.status}`;
  const pct = task.total > 0 ? Math.min(100, (task.downloaded / task.total) * 100) : 0;
  const selectTask = useAppStore((s) => s.selectTask);
  const running = task.status === "downloading" || task.status === "postprocess" || task.status === "starting";
  const indeterminate = running && task.total <= 0;
  const color =
    task.status === "failed" ? "text-red-400" : task.status === "done" ? "text-emerald-300" : "text-sky-300";

  return (
    <div className="rounded-lg border border-zinc-800 bg-zinc-900/60 p-3" onClick={() => selectTask(id)}>
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm text-zinc-100" title={task.url}>
            {task.title ?? task.url}
          </p>
          <p className="mt-0.5 flex items-center gap-2 text-xs text-zinc-500">
            <span className={color}>{t(stKey)}</span>
            {running && (
              <>
                <span>{fmtSpeed(task.speed)}</span>
                {task.eta > 0 && <span>ETA {fmtEta(task.eta)}</span>}
                {task.total > 0 && (
                  <span>
                    {fmtSize(task.downloaded)} / {fmtSize(task.total)}
                  </span>
                )}
              </>
            )}
            {task.status === "failed" && task.error && (
              <span className="truncate text-red-400/80" title={task.error}>
                {task.error}
              </span>
            )}
          </p>
        </div>
        {running && (
            <button onClick={() => void pause(id)} className="rounded-md border border-zinc-700 p-1.5 text-zinc-300 hover:bg-zinc-800" title={t("action.pause")}>
              <Pause className="h-3.5 w-3.5" />
            </button>
        )}
        {canCancelStatus(task.status) && (
            <button onClick={() => void cancel(id)} className="rounded-md border border-zinc-700 p-1.5 text-zinc-300 hover:bg-zinc-800" title={t("action.cancel")}>
              <X className="h-3.5 w-3.5" />
            </button>
        )}
        {task.status === "paused" && (
          <button onClick={() => void resume(id)} className="rounded-md border border-zinc-700 p-1.5 text-zinc-300 hover:bg-zinc-800" title={t("action.resume")}>
            <Play className="h-3.5 w-3.5" />
          </button>
        )}
        {task.status === "done" && task.filePath && (
          <button onClick={() => void openFolder(task.filePath!)} className="rounded-md border border-zinc-700 p-1.5 text-zinc-300 hover:bg-zinc-800" title={t("action.openFolder")}>
            <FolderOpen className="h-3.5 w-3.5" />
          </button>
        )}
        {(task.status === "failed" || task.status === "canceled") && (
          <button onClick={() => void retry(id)} className="rounded-md border border-zinc-700 p-1.5 text-zinc-300 hover:bg-zinc-800" title={t("action.retry")}>
            <RotateCcw className="h-3.5 w-3.5" />
          </button>
        )}
        {canRemoveStatus(task.status) ? (
          <button onClick={() => void remove(id)} className="rounded-md border border-zinc-800 p-1.5 text-zinc-500 hover:bg-zinc-800" title={t("action.remove")}>
            <Trash2 className="h-3.5 w-3.5" />
          </button>
        ) : null}
      </div>
      <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-zinc-800">
        <div
          className={`h-full rounded-full ${
            task.status === "failed" ? "bg-red-500/70" : task.status === "done" ? "bg-emerald-500" : "bg-sky-500"
          } ${indeterminate ? "w-1/3 animate-pulse" : ""}`}
          style={indeterminate ? undefined : { width: `${task.status === "done" ? 100 : pct}%` }}
        />
      </div>
    </div>
  );
}

export default function TaskTable() {
  const tasks = useAppStore((s) => s.tasks);
  const t = useT();
  return (
    <div className="flex h-full min-h-[220px] flex-col">
      <div className="mb-2 flex items-center gap-2 px-1">
        <Download className="h-4 w-4 text-sky-400" />
        <h2 className="text-sm font-semibold text-zinc-200">{t("tasks.title")}</h2>
        <span className="text-xs text-zinc-500">{tasks.length}</span>
      </div>
      <div className="min-h-0 flex-1 space-y-2 overflow-y-auto">
        {tasks.length === 0 && <p className="px-1 text-xs text-zinc-600">{t("tasks.empty")}</p>}
        {tasks.map((row) => (
          <TaskRow key={row.id} id={row.id} />
        ))}
      </div>
    </div>
  );
}
