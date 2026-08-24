# Bump bundled yt-dlp / ffmpeg

Stable releases **must** use `engines.lock.json`. Do not point Stable at `latest`.

1. Pick versions (yt-dlp tag + ffmpeg build).
2. Fill `url` + `sha256` per platform in `engines.lock.json`.
3. Locally: `bash scripts/fetch-engines.sh --lock` (or `powershell -File scripts/fetch-engines.ps1 -Lock`).
4. Confirm `--version` / `-version` and commit the lock file.
