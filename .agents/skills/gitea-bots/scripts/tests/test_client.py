"""Tests for the thread-safe Gitea client session handling (#138)."""

import threading
import unittest

from gitea_bots.client import GiteaClient


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

    def test_delete_uses_the_shared_request_path(self):
        fake = _FakeSession()
        client = GiteaClient("https://gitea.test/api/v1", "secret", session=fake)

        self.assertEqual(client.delete("/repos/Atom/example/issues/1"), {"ok": True})
        method, url, kwargs = fake.calls[0]
        self.assertEqual(method, "DELETE")
        self.assertEqual(url, "https://gitea.test/api/v1/repos/Atom/example/issues/1")
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


if __name__ == "__main__":
    unittest.main()
