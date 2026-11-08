"""Tests for the shared Gitea plumbing (client, remote, .env)."""

import os
import tempfile
import threading
import unittest
from pathlib import Path
from unittest.mock import patch

from issue_labels.gitea import GiteaClient, derive_remote, load_dotenv


class _Response:
    status_code = 200
    content = b"{}"

    @staticmethod
    def json():
        return {"ok": True}


class _FakeSession:
    def __init__(self):
        self.calls = []

    def request(self, method, url, **kwargs):
        self.calls.append((method, url, kwargs))
        return _Response()


class ClientSessionTests(unittest.TestCase):
    def test_injected_session_is_used_as_is(self):
        fake = _FakeSession()
        client = GiteaClient("https://gitea.test/api/v1", "secret", session=fake)
        self.assertIs(client._get_session(), fake)

        self.assertEqual(client.get("/user"), {"ok": True})
        method, url, kwargs = fake.calls[0]
        self.assertEqual(method, "GET")
        self.assertEqual(url, "https://gitea.test/api/v1/user")
        self.assertEqual(kwargs["headers"]["Authorization"], "token secret")

    def test_sessions_are_per_thread(self):
        client = GiteaClient("https://gitea.test/api/v1", "secret")
        main_session = client._get_session()
        self.assertIs(client._get_session(), main_session)

        captured = {}
        ready = threading.Barrier(2)

        def capture(name):
            captured[name] = client._get_session()
            ready.wait(timeout=5)

        threads = [
            threading.Thread(target=capture, args=(name,)) for name in ("a", "b")
        ]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join(timeout=5)

        self.assertIn("a", captured)
        self.assertIn("b", captured)
        self.assertIsNot(captured["a"], captured["b"])
        self.assertIsNot(captured["a"], main_session)
        self.assertIsNot(captured["b"], main_session)


class RemoteTests(unittest.TestCase):
    def test_ssh_url_with_port(self):
        self.assertEqual(
            derive_remote("ssh://git@git.tokyofy.top:33022/Atom/openrouter-image-runner.git"),
            ("git.tokyofy.top", "Atom/openrouter-image-runner"),
        )

    def test_scp_style(self):
        self.assertEqual(
            derive_remote("git@gitea.test:Atom/example.git"),
            ("gitea.test", "Atom/example"),
        )

    def test_https_url(self):
        self.assertEqual(
            derive_remote("https://gitea.test/Atom/example.git"),
            ("gitea.test", "Atom/example"),
        )


class DotenvTests(unittest.TestCase):
    def test_quotes_comments_export_and_expansion(self):
        with tempfile.TemporaryDirectory() as tmp:
            env_file = Path(tmp) / ".env"
            env_file.write_text(
                "# a comment\n"
                "GITEA_TOKEN='secret'\n"
                "export GITEA_SELF=\"testbot\"\n"
                "FLAG=${GITEA_FLAG:-0}\n"
                "EMPTY=\n",
                encoding="utf-8",
            )
            with patch.dict(os.environ, {"GITEA_FLAG": "1"}):
                values = load_dotenv(env_file)
            self.assertEqual(values["GITEA_TOKEN"], "secret")
            self.assertEqual(values["GITEA_SELF"], "testbot")
            self.assertEqual(values["FLAG"], "1")  # expanded from process env
            self.assertEqual(values["EMPTY"], "")

    def test_missing_file_returns_empty(self):
        self.assertEqual(load_dotenv(None), {})


if __name__ == "__main__":
    unittest.main()
