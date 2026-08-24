import { describe, expect, it } from "vitest";
import { customFormatFromSelection, formatOptions, type FormatOption } from "./format";
import type { FormatInfo } from "./ytdlp";

describe("formatOptions", () => {
  it("dedupes format ids and prefers video", () => {
    const formats: FormatInfo[] = [
      { formatId: "ba", ext: "m4a", resolution: null, vcodec: null, acodec: "aac", filesize: 1, filesizeApprox: null, fps: null, dynamicRange: null, protocol: null, container: null, note: null, language: null },
      { formatId: "bv", ext: "mp4", resolution: "1280x720", vcodec: "avc1", acodec: null, filesize: 10, filesizeApprox: null, fps: 30, dynamicRange: null, protocol: null, container: null, note: null, language: null },
      { formatId: "bv", ext: "mp4", resolution: "1280x720", vcodec: "avc1", acodec: null, filesize: 10, filesizeApprox: null, fps: 30, dynamicRange: null, protocol: null, container: null, note: null, language: null },
    ];
    const opts = formatOptions(formats);
    expect(opts[0].formatId).toBe("bv");
    expect(opts).toHaveLength(2);
  });
});

describe("customFormatFromSelection", () => {
  it("adds +ba for video-only", () => {
    const o: FormatOption = { formatId: "137", label: "1080p", sub: [], vcodec: "avc1", acodec: null, resolution: "1920x1080", height: 1080 };
    expect(customFormatFromSelection(o)).toEqual({ preset: "custom", customFormat: "137+ba/137/b" });
  });
});
