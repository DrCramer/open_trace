"""Desktop-style workspace pages: whitelist serving, session token and strings."""
from pathlib import Path
import re
import subprocess
import sys
import unittest
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
WEB = ROOT / "genemapper_open" / "web" / "idx"


class IdxPagesTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.process = subprocess.Popen([sys.executable, "-u", "-m", "genemapper_open", "serve", "--port", "0"],
                                       cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                       text=True, encoding="utf-8")
        line = cls.process.stdout.readline()
        match = re.search(r"http://127\.0\.0\.1:\d+/", line)
        if not match:
            cls.process.terminate(); cls.process.wait(timeout=5)
            raise RuntimeError("Server did not start: " + line)
        cls.base = match.group(0)
        old = urllib.request.urlopen(cls.base, timeout=5).read().decode("utf-8")
        cls.token = re.search(r"const TOKEN='([^']+)'", old).group(1)

    @classmethod
    def tearDownClass(cls):
        cls.process.terminate(); cls.process.wait(timeout=5); cls.process.stdout.close()

    def get(self, path):
        try:
            response = urllib.request.urlopen(self.base + path, timeout=5)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            return response.code, response.headers.get("Content-Type"), response.read()

    def test_project_page_carries_session_token(self):
        for path in ("idx", "idx/", "idx/project.html", "idx/panels.html"):
            with self.subTest(path=path):
                code, kind, body = self.get(path)
                self.assertEqual(code, 200)
                self.assertTrue(kind.startswith("text/html"))
                self.assertIn(f"window.OT_TOKEN='{self.token}'".encode(), body)
                self.assertNotIn(b"__SESSION_TOKEN__", body)

    def test_static_files_and_types(self):
        for name, kind in (("plot.html", "text/html"), ("idx.css", "text/css"), ("i18n.js", "text/javascript"),
                           ("ui.js", "text/javascript"), ("project.js", "text/javascript"), ("plot.js", "text/javascript"),
                           ("kits.js", "text/javascript"), ("panels.js", "text/javascript"),
                           ("methods.js", "text/javascript"), ("tables.js", "text/javascript"),
                           ("sizestd.js", "text/javascript"), ("sizematch.js", "text/javascript"),
                           ("options.js", "text/javascript"), ("reports.js", "text/javascript"),
                           ("report.js", "text/javascript"), ("print.js", "text/javascript"),
                           ("report.html", "text/html"), ("print.html", "text/html"),
                           ("profiles.js", "text/javascript")):
            with self.subTest(name=name):
                code, got, body = self.get("idx/" + name)
                self.assertEqual(code, 200)
                self.assertTrue(got.startswith(kind))
                self.assertEqual(body, (WEB / name).read_bytes())

    def test_only_whitelisted_paths_are_served(self):
        for path in ("idx/../server.py", "idx/unknown.js", "idx/%2e%2e/server.py", "idx/project.js?x=1", "idx//ui.js"):
            with self.subTest(path=path):
                self.assertEqual(self.get(path)[0], 404)

    def test_russian_and_english_strings_have_the_same_keys(self):
        source = (WEB / "i18n.js").read_text(encoding="utf-8")
        en = source[source.index("en:{"):source.index("ru:{")]
        ru = source[source.index("ru:{"):source.index("}};")]
        keys = lambda block: set(re.findall(r"(?:^|[{,])\s*([A-Za-z][A-Za-z0-9_]*):'", block, re.M))
        self.assertGreater(len(keys(en)), 150)
        self.assertEqual(keys(en), keys(ru))
        used = set()
        for name in ("project.js", "plot.js", "panels.js", "ui.js", "methods.js", "tables.js", "sizestd.js", "sizematch.js", "options.js", "reports.js", "report.js", "print.js", "profiles.js"):
            used |= set(re.findall(r"\b(?:T|fmt)\('([A-Za-z_][A-Za-z0-9_]*)'[),]", (WEB / name).read_text(encoding="utf-8")))
        self.assertEqual(sorted(used - keys(en)), [])


if __name__ == "__main__":
    unittest.main()
