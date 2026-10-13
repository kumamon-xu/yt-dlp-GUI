import importlib.util
import pathlib
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location("fetch_m3u8dl", pathlib.Path(__file__).with_name("fetch-m3u8dl.py"))
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class MetadataAuthenticationTests(unittest.TestCase):
    def request_for(self, url, environment):
        with mock.patch.dict(MODULE.os.environ, environment, clear=True), mock.patch.object(MODULE.urllib.request, "urlopen") as open_url:
            open_url.return_value.__enter__.return_value.read.return_value = b"data"
            self.assertEqual(MODULE.download(url), b"data")
            return open_url.call_args.args[0]

    def test_ci_authenticates_public_release_metadata(self):
        for name in ("GH_TOKEN", "GITHUB_TOKEN"):
            with self.subTest(variable=name):
                request = self.request_for("https://api.github.com/repos/nilaoda/N_m3u8DL-RE/releases/latest", {name: "test-token"})
                self.assertEqual(request.get_header("Authorization"), "Bearer test-token")

    def test_token_is_not_sent_to_asset_hosts_or_insecure_urls(self):
        for url in ("https://github.com/nilaoda/N_m3u8DL-RE/releases/download/v0.6.0-beta/asset.zip",
                    "https://release-assets.githubusercontent.com/asset.zip",
                    "https://api.github.com.example.com/asset.zip", "http://api.github.com/repos/test"):
            with self.subTest(url=url):
                self.assertIsNone(self.request_for(url, {"GH_TOKEN": "test-token"}).get_header("Authorization"))

    def test_local_download_works_without_authentication(self):
        self.assertIsNone(self.request_for("https://api.github.com/repos/test", {}).get_header("Authorization"))


if __name__ == "__main__":
    unittest.main()
