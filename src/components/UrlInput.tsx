import { useEffect, useRef, useState } from "react";
import { Link2, Loader2, Search } from "lucide-react";
import { useAppStore } from "../store";

const isUrl = (s: string) => /^https?:\/\/\S+$/i.test(s.trim());

export default function UrlInput() {
  const [value, setValue] = useState("");
  const preview = useAppStore((s) => s.preview);
  const status = useAppStore((s) => s.previewStatus);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const submit = (url: string) => {
    const u = url.trim();
    if (!isUrl(u)) return;
    setValue(u);
    void preview(u);
  };

  // 粘贴合法 URL 后 600ms 防抖自动解析
  useEffect(() => {
    if (!isUrl(value)) return;
    timer.current = setTimeout(() => void preview(value.trim()), 600);
    return () => {
      if (timer.current) clearTimeout(timer.current);
    };
  }, [value, preview]);

  return (
    <div className="flex items-center gap-2">
      <div className="relative flex-1">
        <Link2 size={15} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-slate-500" />
        <input
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && submit(value)}
          placeholder="粘贴视频 / 播放列表 / 合集链接（B站、YouTube、抖音、TikTok…）"
          className="w-full rounded-lg border border-slate-700 bg-slate-900 py-2.5 pl-9 pr-3 text-sm text-slate-200 placeholder:text-slate-600 focus:border-sky-500 focus:outline-none focus:ring-1 focus:ring-sky-500/40"
        />
      </div>
      <button
        onClick={() => submit(value)}
        disabled={!isUrl(value) || status === "loading"}
        className="flex items-center gap-1.5 rounded-lg bg-sky-600 px-4 py-2.5 text-sm font-medium text-white transition hover:bg-sky-500 disabled:cursor-not-allowed disabled:opacity-40"
      >
        {status === "loading" ? <Loader2 size={14} className="animate-spin" /> : <Search size={14} />}
        解析
      </button>
    </div>
  );
}
