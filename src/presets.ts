export const PRESET_IDS = ["mp4", "best", "1080p", "720p", "mp3", "m4a", "custom"] as const;
export type PresetId = (typeof PRESET_IDS)[number];

export const DEFAULT_PRESET: PresetId = "mp4";
