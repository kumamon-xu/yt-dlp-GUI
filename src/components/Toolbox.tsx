import { FileJson, Image, Subtitles } from "lucide-react";
import { useAppStore, useT } from "../store";

export default function Toolbox() {
  const t = useT();
  const url = useAppStore((s) => s.previewUrl);
  const enqueue = useAppStore((s) => s.enqueueUrls);

  const run = (extra: Parameters<typeof enqueue>[1]) => {
    if (!url) return;
    void enqueue(url, extra);
  };

  const btn = "flex flex-1 items-center justify-center gap-1.5 rounded-md border border-slate-700 px-2 py-1.5 text-xs text-slate-300 hover:bg-slate-800 disabled:opacity-40";

  return (
    <div className="rounded-xl border border-slate-800 bg-slate-900/40 p-3">
      <h3 className="mb-2 text-xs font-semibold text-slate-300">{t("tools.title")}</h3>
      <div className="flex gap-2">
        <button
          className={btn}
          disabled={!url}
          onClick={() => run({ skipDownload: true, writeSubs: true, convertSubs: "srt", noPlaylist: true, kind: "subtitles" })}
        >
          <Subtitles size={13} /> {t("tools.subs")}
        </button>
        <button
          className={btn}
          disabled={!url}
          onClick={() => run({ skipDownload: true, writeThumbnail: true, noPlaylist: true, kind: "thumbnail" })}
        >
          <Image size={13} /> {t("tools.thumb")}
        </button>
        <button
          className={btn}
          disabled={!url}
          onClick={() => run({ skipDownload: true, writeInfoJson: true, noPlaylist: true, kind: "metadata" })}
        >
          <FileJson size={13} /> {t("tools.meta")}
        </button>
      </div>
    </div>
  );
}
