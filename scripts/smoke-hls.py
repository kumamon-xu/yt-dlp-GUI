#!/usr/bin/env python3
"""Run the real Rust HLS pipeline against local, authenticated HLS fixtures."""
import functools
import http.server
import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import threading

ROOT = pathlib.Path(__file__).resolve().parents[1]
FFMPEG = ROOT / "code" / ("ffmpeg.exe" if os.name == "nt" else "ffmpeg")
CREATE_NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0


def ffmpeg(*args):
    subprocess.run([str(FFMPEG), "-nostdin", "-y", "-hide_banner", "-loglevel", "error", *args], check=True,
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


class FailedCdnHandler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_error(503, "fixture CDN unavailable")

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
        # Genuine CBR HLS, rather than repeating TS packets/timestamps. The latter
        # confuses media probing in some bundled FFmpeg builds.
        ffmpeg("-f", "lavfi", "-i", "testsrc2=size=160x90:rate=10", "-t", "16", "-an", "-c:v", "libx264",
               "-b:v", "4M", "-minrate", "4M", "-maxrate", "4M", "-bufsize", "8M", "-x264-params", "nal-hrd=cbr",
               "-pix_fmt", "yuv420p", "-g", "10", "-hls_time", "1", "-hls_list_size", "0", str(fixture / "rate.m3u8"))
        rate_bytes = sum(p.stat().st_size for p in fixture.glob("rate*.ts"))
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
        bad_server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), FailedCdnHandler)
        threading.Thread(target=bad_server.serve_forever, daemon=True).start()
        bad_origin = f"http://127.0.0.1:{bad_server.server_port}"
        good_host = f"127.0.0.1:{server.server_port}"
        same_stream = 'BANDWIDTH=200000,RESOLUTION=160x90,FRAME-RATE=10.000,CODECS="avc1.42e00a"'
        (fixture / "auto-master.m3u8").write_text(f'#EXTM3U\n#EXT-X-STREAM-INF:{same_stream},STABLE-VARIANT-ID="video"\n{bad_origin}/video.m3u8\n'
            f'#EXT-X-STREAM-INF:{same_stream},STABLE-VARIANT-ID="video"\nvideo.m3u8\n')
        (fixture / "auto-steering.m3u8").write_text('#EXTM3U\n#EXT-X-CONTENT-STEERING:SERVER-URI="steering.json",PATHWAY-ID="failed"\n'
            f'#EXT-X-STREAM-INF:{same_stream},PATHWAY-ID="failed"\n{bad_origin}/video.m3u8\n')
        (fixture / "steering.json").write_text(json.dumps({"VERSION": 1, "TTL": 60, "PATHWAY-PRIORITY": ["mirror"],
            "PATHWAY-CLONES": [{"BASE-ID": "failed", "ID": "mirror", "URI-REPLACEMENT": {"HOST": good_host}}]}))
        (fixture / "video").mkdir()
        for payload in [*fixture.glob("*.ts"), fixture / "enc.key"]:
            shutil.copyfile(payload, fixture / "video" / payload.name)
        for source, target in [("video.m3u8", "mirror.m3u8"), ("encrypted.m3u8", "mirror-encrypted.m3u8")]:
            text = (fixture / source).read_text()
            text = text.replace('URI="enc.key"', f'URI="{bad_origin}/video/enc.key"')
            text = '\n'.join(f'{bad_origin}/video/{line}' if line and not line.startswith('#') else line for line in text.splitlines()) + '\n'
            (fixture / target).write_text(text)
        env = {**os.environ, "TEST_HLS_URL": f"http://127.0.0.1:{server.server_port}", "TEST_HLS_COOKIES": str(fixture / "cookies.txt"),
               "TEST_HLS_RATE_BYTES": str(rate_bytes), "TEST_HLS_BAD_ORIGIN": bad_origin}
        try:
            result = subprocess.run(["cargo", "test", "--manifest-path", str(ROOT / "src-tauri/Cargo.toml"), "--lib",
                "real_hls_", "--", "--ignored", "--nocapture"], env=env, cwd=ROOT, creationflags=CREATE_NO_WINDOW,
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            print(result.stdout.decode("utf-8", "replace"))
            return result.returncode
        finally:
            server.shutdown()
            server.server_close()
            bad_server.shutdown()
            bad_server.server_close()


if __name__ == "__main__":
    raise SystemExit(main())
