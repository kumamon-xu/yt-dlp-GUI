import { useEffect } from "react";
import { Copy, Terminal } from "lucide-react";
import { useAppStore, useT } from "../store";

export default function CommandBar() {
  const cmd = useAppStore((s) => s.commandPreview);
  const refresh = useAppStore((s) => s.refreshCommand);
  const options = useAppStore((s) => s.options);
  const previewUrl = useAppStore((s) => s.previewUrl);
  const t = useT();

  useEffect(() => {
    void refresh();
  }, [options, previewUrl, refresh]);

  return (
    <div className="mt-2 flex items-center gap-2 rounded-md border border-slate-800 bg-slate-950/60 px-2 py-1.5">
      <Terminal size={12} className="shrink-0 text-slate-500" />
      <code className="min-w-0 flex-1 truncate font-mono text-[11px] text-slate-400" title={`${cmd}\n${t("cmd.disclaimer")}`}>
        {cmd || t("cmd.title")}
      </code>
      <button
        onClick={() => cmd && void navigator.clipboard.writeText(cmd)}
        className="shrink-0 rounded p-1 text-slate-400 hover:bg-slate-800 hover:text-slate-100"
        title={t("action.copy")}
      >
        <Copy size={12} />
      </button>
    </div>
  );
}
