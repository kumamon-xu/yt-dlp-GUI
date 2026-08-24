import importlib.util
import pathlib
import tempfile
import unittest


SCRIPT = pathlib.Path(__file__).with_name("prepare-release-assets.py")
SPEC = importlib.util.spec_from_file_location("prepare_release_assets", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


class CopyUniqueTests(unittest.TestCase):
    def test_normalizes_spaces_before_checksums_and_upload(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            source = root / "yt-dlp GUI_0.2.0_x64-setup.exe"
            source.write_bytes(b"installer")
            out = root / "publish"
            out.mkdir()

            copied = MODULE.copy_unique(source, out, set())

            self.assertEqual(copied.name, "yt-dlp.GUI_0.2.0_x64-setup.exe")
            self.assertEqual(copied.read_bytes(), b"installer")


if __name__ == "__main__":
    unittest.main()
