"""Exercise the real loopback HTTP server, including upload boundary checks."""
import json
from pathlib import Path
import re
import subprocess
import sys
import unittest
import urllib.error
import urllib.request

from genemapper_open.demo import demo_bytes


class HttpTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        root = Path(__file__).resolve().parents[1]
        cls.process = subprocess.Popen([sys.executable, "-u", "-m", "genemapper_open", "serve", "--port", "0"],
                                       cwd=root, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                       text=True, encoding="utf-8")
        line = cls.process.stdout.readline()
        match = re.search(r"http://127\.0\.0\.1:\d+/", line)
        if not match:
            cls.process.terminate()
            cls.process.wait(timeout=5)
            raise RuntimeError("Test server did not start: " + line)
        cls.url = match.group(0)
        cls.page = urllib.request.urlopen(cls.url, timeout=5).read().decode("utf-8")
        cls.token = re.search(r"const TOKEN='([^']+)'", cls.page).group(1)

    @classmethod
    def tearDownClass(cls):
        cls.process.terminate()
        cls.process.wait(timeout=5)
        cls.process.stdout.close()
        cls.process.stderr.close()

    def post(self, path, data=b"", token=True, extra=None):
        headers = {"Content-Type": "application/octet-stream"}
        if token:
            headers["X-Session-Token"] = self.token
        headers.update(extra or {})
        request = urllib.request.Request(self.url + path, data=data, headers=headers, method="POST")
        try:
            response = urllib.request.urlopen(request, timeout=5)
        except urllib.error.HTTPError as response:
            with response:
                return response.code, json.load(response)
        with response:
            return response.status, json.load(response)

    def test_page_and_health(self):
        self.assertIn("<title>OpenTrace", self.page)
        with urllib.request.urlopen(self.url + "api/health", timeout=5) as response:
            self.assertEqual(json.load(response)["status"], "ok")

    def test_demo_request(self):
        code, result = self.post("api/demo")
        self.assertEqual(code, 200)
        self.assertEqual(len(result["channels"]), 6)
        self.assertEqual(sum(len(c["peaks"]) for c in result["channels"]), 24)

    def test_upload_and_calibration(self):
        code, result = self.post("api/analyze?filename=sample.fsa&anchors=0:1,2999:600", demo_bytes())
        self.assertEqual(code, 200)
        self.assertGreater(result["channels"][0]["peaks"][0]["size_bp"], 1)

    def test_bad_upload(self):
        code, result = self.post("api/analyze", b"not ABIF")
        self.assertEqual(code, 400)
        self.assertIn("ABIF", result["error"])

    def test_automatic_standard_report(self):
        query = ("api/demo-sizing?sizing_mode=auto&standard_channel=DATA106"
                 "&standard_bp=75,105,155,205,275,335,425,505,605")
        code, result = self.post(query)
        self.assertEqual(code, 200)
        self.assertEqual(result["sizing"]["status"], "accepted")
        self.assertEqual(len(result["calibration"]), 9)
        self.assertEqual(len(result["source_sha256"]), 64)

    def test_rejected_standard_preserves_unsized_signal(self):
        code, result = self.post("api/demo-sizing?sizing_mode=auto&standard_channel=DATA106"
                                 "&standard_bp=75,105,155,205,275,335,425,505,605&standard_end=2400")
        self.assertEqual(code, 200)
        self.assertEqual(result["sizing"]["status"], "rejected")
        self.assertEqual(result["calibration"], [])
        self.assertTrue(all("size_bp" not in p for c in result["channels"] for p in c["peaks"]))

    def test_automatic_invalid_configuration(self):
        for query in ("sizing_mode=auto&standard_channel=missing&standard_bp=75,100,150,200",
                      "sizing_mode=auto&standard_channel=DATA106&standard_bp=75,75,150,200",
                      "sizing_mode=manual", "sizing_mode=auto&max_residual_bp=nan"):
            with self.subTest(query=query):
                code, _ = self.post("api/demo-sizing?" + query)
                self.assertEqual(code, 400)

    def test_selected_calibration_model_and_diagnostics(self):
        query = ("api/demo-sizing?sizing_mode=auto&standard_channel=DATA106"
                 "&standard_bp=75,105,155,205,275,335,425,505,605&calibration_model=lsq3")
        code, result = self.post(query)
        self.assertEqual(code, 200)
        self.assertEqual(result["sizing"]["status"], "accepted")
        self.assertEqual(result["sizing"]["calibration_model"], "lsq3")
        self.assertTrue(result["sizing"]["calibration_diagnostics"]["monotonic"])

    def test_unknown_calibration_model(self):
        code, _ = self.post("api/demo-sizing?calibration_model=unknown")
        self.assertEqual(code, 400)

    def test_bad_threshold(self):
        code, _ = self.post("api/demo?threshold=nan")
        self.assertEqual(code, 400)

    def test_no_session_token(self):
        code, _ = self.post("api/demo", token=False)
        self.assertEqual(code, 403)

    def test_cross_origin_rejected(self):
        code, _ = self.post("api/demo", extra={"Origin": "https://example.com"})
        self.assertEqual(code, 403)

    def test_oversized_body_rejected_before_reading(self):
        code, _ = self.post("api/analyze", extra={"Content-Length": str(64 * 1024 * 1024 + 1)})
        self.assertEqual(code, 413)

    def test_json_requests_above_upload_limit_accepted(self):
        # a run cohort (sample + all ladders of the folder) can exceed the 64 MiB FSA upload limit
        body = b'{"action":"session","pad":"' + b"x" * (65 * 1024 * 1024) + b'"}'
        code, data = self.post("api/security", data=body, extra={"Content-Type": "application/json"})
        self.assertEqual(code, 200, data)

    def test_unexpected_server_error_is_reported(self):
        # an unhandled exception type must still answer with JSON instead of dropping the connection
        code, data = self.post("api/str-run", data=b'{"sample": 1, "ladders": 2, "kit": []}',
                               extra={"Content-Type": "application/json"})
        self.assertIn(code, (400, 500))
        self.assertIn("error", data)


if __name__ == "__main__":
    unittest.main()
