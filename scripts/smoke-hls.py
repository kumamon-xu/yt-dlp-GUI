#!/usr/bin/env python3
"""Run the real Rust HLS pipeline against local, authenticated HLS fixtures."""
import functools
import http.server
import math
import os
import pathlib
import subprocess
import tempfile
import threading

ROOT = pathlib.Path(__file__).resolve().parents[1]
FFMPEG = ROOT / "code" / ("ffmpeg.exe" if os.name == "nt" else "ffmpeg")
CREATE_NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0


def ffmpeg(*args):
    subprocess.run([str(FFMPEG), "-hide_banner", "-loglevel", "error", *args], check=True,
                   creationflags=CREATE_NO_WINDOW)


class FixtureHandler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.path.split("?")[0] != "/watch.html":
            if "sid=demo" not in self.headers.get("Cookie", ""):
                self.send_error(403, "cookie required")
                return
            if "/watch.html" not in self.headers.get("Referer", ""):
                self.send_error(403, "referer required")
                return
        super().do_GET()

    def log_message(self, *args):
        pass


def main():
    with tempfile.TemporaryDirectory(prefix="ytdlp-hls-smoke-") as temporary:
        fixture = pathlib.Path(temporary)
        ffmpeg("-f", "lavfi", "-i", "testsrc=size=160x90:rate=10", "-t", "3", "-an", "-c:v", "libx264",
               "-pix_fmt", "yuv420p", "-g", "10", "-hls_time", "1", "-hls_list_size", "0", str(fixture / "video.m3u8"))
        ffmpeg("-f", "lavfi", "-i", "sine=frequency=440:sample_rate=44100", "-t", "3", "-vn", "-c:a", "aac",
               "-hls_time", "1", "-hls_list_size", "0", str(fixture / "audio.m3u8"))
        ffmpeg("-f", "lavfi", "-i", "testsrc=size=160x90:rate=20", "-t", "3", "-an", "-c:v", "libx265",
               "-x265-params", "log-level=error:pools=1:frame-threads=1", "-pix_fmt", "yuv420p", "-g", "20",
               "-hls_time", "1", "-hls_list_size", "0", str(fixture / "video-alt.m3u8"))
        (fixture / "master.m3u8").write_text('#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="audio",NAME="English",DEFAULT=YES,AUTOSELECT=YES,URI="audio.m3u8"\n'
            '#EXT-X-STREAM-INF:BANDWIDTH=200000,RESOLUTION=160x90,FRAME-RATE=10.000,CODECS="avc1.42e00a,mp4a.40.2",AUDIO="audio"\nvideo.m3u8\n'
            '#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=160x90,FRAME-RATE=20.000,CODECS="hev1.1.6.L30.B0,mp4a.40.2",AUDIO="audio"\nvideo-alt.m3u8\n')
        # Valid TS payloads large enough to distinguish 1 MiB/s from the erroneous 8.192 MiB/s.
        block = (fixture / "video0.ts").read_bytes()
        rate_segment = block * math.ceil(512 * 1024 / len(block))
        for i in range(8):
            (fixture / f"rate{i}.ts").write_bytes(rate_segment)
        (fixture / "rate.m3u8").write_text('#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n'
            + ''.join(f'#EXTINF:1.0,\nrate{i}.ts\n' for i in range(8)) + '#EXT-X-ENDLIST\n')
        (fixture / "enc.key").write_bytes(bytes(range(16)))
        (fixture / "keyinfo.txt").write_text(f"enc.key\n{fixture / 'enc.key'}\n")
        ffmpeg("-f", "lavfi", "-i", "testsrc=size=160x90:rate=10", "-f", "lavfi", "-i", "sine=frequency=440",
               "-t", "3", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-g", "10", "-c:a", "aac",
               "-hls_time", "1", "-hls_list_size", "0", "-hls_key_info_file", str(fixture / "keyinfo.txt"), str(fixture / "encrypted.m3u8"))
        (fixture / "watch.html").write_text('<html><title>Fixture.with.dots</title><video controls src="master.m3u8"></video></html>')
        (fixture / "cookies.txt").write_text('# Netscape HTTP Cookie File\n127.0.0.1\tFALSE\t/\tFALSE\t0\tsid\tdemo\n')
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(FixtureHandler, directory=str(fixture)))
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        env = {**os.environ, "TEST_HLS_URL": f"http://127.0.0.1:{server.server_port}", "TEST_HLS_COOKIES": str(fixture / "cookies.txt"),
               "TEST_HLS_RATE_BYTES": str(8 * len(rate_segment))}
        try:
            result = subprocess.run(["cargo", "test", "--manifest-path", str(ROOT / "src-tauri/Cargo.toml"), "--lib",
                "real_hls_", "--", "--ignored", "--nocapture"], env=env, cwd=ROOT, creationflags=CREATE_NO_WINDOW,
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            print(result.stdout.decode("utf-8", "replace"))
            return result.returncode
        finally:
            server.shutdown()
            server.server_close()


if __name__ == "__main__":
    raise SystemExit(main())
