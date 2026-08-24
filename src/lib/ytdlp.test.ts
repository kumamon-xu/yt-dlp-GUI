import { describe, expect, it } from "vitest";
import {
  acceptTaskUpdate,
  canCancelStatus,
  canRemoveStatus,
  compressPlaylistItems,
  isCurrentToken,
  redactUserinfo,
  resolveDownloadPreset,
  retryTaskRequest,
  settingsDraftDirty,
  splitUrls,
  toolboxTask,
  type GlobalSettings,
  type NewTask,
} from "./ytdlp";

const base = (): GlobalSettings => ({
  defaultPreset: "mp4",
  outDir: "",
  outTemplate: "%(title)s [%(id)s].%(ext)s",
  concurrentFragments: 4,
  maxConcurrentTasks: 2,
  limitRate: null,
  cookiesBrowser: null,
  cookiesFile: null,
  proxy: null,
  enginePath: null,
  ffmpegPath: null,
  mergeFormat: "mp4",
});

describe("splitUrls", () => {
  it("splits whitespace and commas", () => {
    expect(splitUrls("https://a.com/x https://b.com/y\nhttps://c.com/z")).toEqual([
      "https://a.com/x",
      "https://b.com/y",
      "https://c.com/z",
    ]);
  });
  it("drops junk", () => {
    expect(splitUrls("not-a-url ftp://x https://ok.com")).toEqual(["https://ok.com"]);
  });
});

describe("resolveDownloadPreset", () => {
  it("uses extra, then session, then default", () => {
    expect(resolveDownloadPreset("1080p", "best", "mp4")).toBe("1080p");
    expect(resolveDownloadPreset(undefined, "best", "mp4")).toBe("best");
    expect(resolveDownloadPreset(undefined, "", "mp3")).toBe("mp3");
  });
});

describe("settingsDraftDirty", () => {
  it("is false until a field changes (no save-on-keystroke helper)", () => {
    const a = base();
    const b = base();
    expect(settingsDraftDirty(a, b)).toBe(false);
    b.defaultPreset = "best";
    expect(settingsDraftDirty(a, b)).toBe(true);
  });
});

describe("retryTaskRequest", () => {
  it("keeps the original request and sets resume", () => {
    const request: NewTask = {
      url: "https://old.example/v",
      preset: "1080p",
      proxy: "http://127.0.0.1:7890",
      noPlaylist: true,
    };
    expect(retryTaskRequest({ url: "https://new.example/v", request })).toEqual({
      ...request,
      url: "https://new.example/v",
      resume: true,
    });
  });
});

describe("isCurrentToken", () => {
  it("drops stale preview results", () => {
    expect(isCurrentToken(1, 2)).toBe(false);
    expect(isCurrentToken(3, 3)).toBe(true);
  });
});

describe("queued buttons", () => {
  it("allows cancel and remove on queued", () => {
    expect(canCancelStatus("queued")).toBe(true);
    expect(canRemoveStatus("queued")).toBe(true);
    expect(canRemoveStatus("downloading")).toBe(false);
  });
});

describe("acceptTaskUpdate", () => {
  it("ignores ids that were removed locally", () => {
    const known = new Set(["a"]);
    expect(acceptTaskUpdate(known, "a")).toBe(true);
    expect(acceptTaskUpdate(known, "deleted")).toBe(false);
  });
});

describe("compressPlaylistItems", () => {
  it("compresses consecutive indices", () => {
    expect(compressPlaylistItems([1, 2, 3, 5, 8, 9, 10])).toBe("1-3,5,8-10");
    expect(compressPlaylistItems(Array.from({ length: 200 }, (_, i) => i + 1))).toBe("1-200");
  });
});

describe("redactUserinfo", () => {
  it("masks proxy password", () => {
    expect(redactUserinfo("http://alice:secret@127.0.0.1:7890")).toBe("http://alice:***@127.0.0.1:7890");
  });
});

describe("toolboxTask", () => {
  it("does not inherit writeSubs for metadata", () => {
    const t = toolboxTask("https://x", "metadata", { proxy: "http://h" });
    expect(t.kind).toBe("metadata");
    expect(t.writeInfoJson).toBe(true);
    expect(t.writeSubs).toBe(false);
    expect(t.sponsorblock).toBeUndefined();
  });
});
