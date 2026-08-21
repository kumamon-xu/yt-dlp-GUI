import { useEffect, useRef, useState } from "react";
import { ChevronDown, ChevronUp, Terminal } from "lucide-react";
import { useAppStore, useT } from "../store";

export default function LogConsole() {
  const [open, setOpen] = useState(false);
  const logs = useAppStore((s) => s.logs);
  const t = useT();
  const end = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (open) end.current?.scrollIntoView({ block: "end" });
  }, [logs.length, open]);

  return (
    <div className="shrink-0 border-t border-zinc-800 bg-black/40">
      <button onClick={() => setOpen(!open)} className="flex w-full items-center gap-2 px-4 py-2 text-xs text-zinc-400 hover:text-zinc-200">
        <Terminal className="h-3.5 w-3.5" />
        {t("log.title")}
        <span className="text-zinc-600">{logs.length}</span>
        {open ? <ChevronDown className="ml-auto h-3.5 w-3.5" /> : <ChevronUp className="ml-auto h-3.5 w-3.5" />}
      </button>
      {open && (
        <div className="max-h-40 overflow-y-auto px-4 pb-3 font-mono text-[11px] leading-5">
          {logs.length === 0 && <span className="text-zinc-600">{t("log.empty")}</span>}
          {logs.map((l, i) => (
            <div key={i} className="whitespace-pre-wrap break-all text-zinc-400">
              <span className="text-zinc-600">{i + 1} </span>
              {l.line}
            </div>
          ))}
          <div ref={end} />
        </div>
      )}
    </div>
  );
}
