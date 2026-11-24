"""Tests for the HTTP layer: auth format, error hints, pagination, login fallback."""

import json
import unittest

from tests import support
from gitea_issues import client as client_module
from gitea_issues.client import GiteaClient, GiteaError, Response, current_login
from gitea_issues.env import Settings


class RecordingTransport:
    """A transport that answers one fixed response and records the request."""

    def __init__(self, status=200, obj=None, raw=None):
        self.status = status
        self.raw = raw if raw is not None else (
            b"" if obj is None else json.dumps(obj).encode()
        )
        self.calls = []

    def __call__(self, method, url, headers, body):
        self.calls.append({"method": method, "url": url, "headers": dict(headers),
                           "body": body})
        return Response(self.status, self.raw)


def settings(**overrides) -> Settings:
    base = dict(api_base=support.API_BASE, repo=support.REPO,
                token=support.TOKEN, timeout=5)
    base.update(overrides)
    return Settings(**base)


class AuthTests(unittest.TestCase):
    def test_authorization_header_uses_the_literal_word_token(self):
        transport = RecordingTransport(obj={"full_name": support.REPO})
        GiteaClient(settings(), transport=transport).get("/repos/x/y")
        self.assertEqual(transport.calls[0]["headers"]["Authorization"],
                         f"token {support.TOKEN}")
        self.assertNotIn("Bearer", str(transport.calls[0]["headers"]))

    def test_post_sets_the_json_content_type(self):
        transport = RecordingTransport(obj={"number": 1})
        GiteaClient(settings(), transport=transport).post("/repos/x/y/issues",
                                                         {"title": "t"})
        self.assertEqual(transport.calls[0]["headers"]["Content-Type"],
                         "application/json")
        self.assertEqual(json.loads(transport.calls[0]["body"]), {"title": "t"})

    def test_a_missing_token_is_refused_before_the_request(self):
        transport = RecordingTransport(obj={})
        with self.assertRaises(GiteaError) as caught:
            GiteaClient(settings(token=""), transport=transport).get("/repos/x/y")
        self.assertIn("no token", str(caught.exception).lower())
        self.assertEqual(transport.calls, [])


class QueryTests(unittest.TestCase):
    def test_empty_params_are_dropped_from_the_url(self):
        transport = RecordingTransport(obj=[])
        GiteaClient(settings(), transport=transport).get(
            "/repos/x/y/issues", {"state": "open", "assignee": "", "q": None})
        self.assertEqual(transport.calls[0]["url"],
                         f"{support.API_BASE}/repos/x/y/issues?state=open")

    def test_repo_path_joins_one_slash(self):
        api = GiteaClient(settings())
        self.assertEqual(api.repo_path("issues"), "/repos/Atom/demo/issues")
        self.assertEqual(api.repo_path("/issues/687"), "/repos/Atom/demo/issues/687")
        self.assertEqual(api.repo_path(), "/repos/Atom/demo")


class ErrorMappingTests(unittest.TestCase):
    def assert_hint(self, status, obj, fragment):
        transport = RecordingTransport(status=status, obj=obj)
        with self.assertRaises(GiteaError) as caught:
            GiteaClient(settings(), transport=transport).get("/repos/x/y/issues")
        self.assertEqual(caught.exception.status, status)
        self.assertIn(fragment, caught.exception.hint)
        self.assertIn(fragment, caught.exception.report())

    def test_401_mentions_the_token_header_format(self):
        self.assert_hint(401, {"message": "invalid username, password or token"},
                         "Bearer")

    def test_404_mentions_the_case_sensitive_slug(self):
        self.assert_hint(404, {"message": "not found"}, "case-sensitive")

    def test_403_mentions_scopes(self):
        self.assert_hint(403, {"message": "must have read:user"}, "scope")

    def test_422_mentions_the_milestone_key(self):
        self.assert_hint(422, {"message": "validation failed"}, "milestone")

    def test_non_json_body_is_reported_as_text(self):
        transport = RecordingTransport(status=404, raw=b"404 page not found")
        with self.assertRaises(GiteaError) as caught:
            GiteaClient(settings(), transport=transport).get("/repos/x/y/issues/1")
        self.assertIn("404 page not found", str(caught.exception))

    def test_the_token_never_appears_in_an_error(self):
        transport = RecordingTransport(status=401, obj={"message": "bad"})
        with self.assertRaises(GiteaError) as caught:
            GiteaClient(settings(), transport=transport).get("/repos/x/y")
        self.assertNotIn(support.TOKEN, str(caught.exception))

    def test_empty_success_body_decodes_to_none(self):
        transport = RecordingTransport(status=204, raw=b"")
        self.assertIsNone(GiteaClient(settings(), transport=transport)
                          .delete("/repos/x/y/labels/3"))


class PaginationTests(unittest.TestCase):
    def test_get_all_walks_pages_until_a_short_page(self):
        pages = {1: [{"id": n} for n in range(3)], 2: [{"id": n} for n in range(3)],
                 3: [{"id": 99}]}
        seen = []

        def transport(method, url, headers, body):
            split = url.split("page=")
            page = int(split[1].split("&")[0]) if len(split) > 1 else 1
            seen.append(page)
            return Response(200, json.dumps(pages.get(page, [])).encode())

        items = GiteaClient(settings(), transport=transport).get_all(
            "/repos/x/y/issues", limit=3, max_pages=9)
        self.assertEqual(len(items), 7)
        self.assertEqual(seen, [1, 2, 3])

    def test_get_all_stops_at_max_pages(self):
        def transport(method, url, headers, body):
            return Response(200, json.dumps([{"id": 1}] * 3).encode())

        items = GiteaClient(settings(), transport=transport).get_all(
            "/repos/x/y/issues", limit=3, max_pages=2)
        self.assertEqual(len(items), 6)


class LoginTests(unittest.TestCase):
    def test_user_endpoint_is_preferred(self):
        transport = support.demo_transport(login="bot")
        self.assertEqual(current_login(GiteaClient(settings(), transport)),
                         ("bot", "/user"))

    def test_scope_403_falls_back_to_the_newest_issue_creator(self):
        transport = support.demo_transport(user_scope=False)
        login, source = current_login(GiteaClient(settings(), transport))
        self.assertEqual((login, source), ("jimmy", "fallback-issue"))
        self.assertTrue(transport.find("GET", "/user"))

    def test_nothing_available_returns_an_empty_login(self):
        transport = support.demo_transport(user_scope=False)
        transport.route(f"GET {support.REPO_PATH}/issues", [])
        self.assertEqual(current_login(GiteaClient(settings(), transport)),
                         ("", ""))


class PullRequestDetectionTests(unittest.TestCase):
    def test_pull_request_field_marks_the_item(self):
        self.assertTrue(client_module.is_pull_request({"pull_request": {"url": "x"}}))
        self.assertFalse(client_module.is_pull_request({"pull_request": None}))
        self.assertFalse(client_module.is_pull_request("not a dict"))


if __name__ == "__main__":
    unittest.main()
