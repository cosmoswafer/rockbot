"""Unit tests for the environment layer: .env parsing and remote derivation."""

import tempfile
import unittest
from pathlib import Path

from gitea_issues import env


class ParseEnvTextTests(unittest.TestCase):
    def test_plain_pairs(self):
        self.assertEqual(
            env.parse_env_text("GITEA_TOKEN=abc\nOTHER=1\n"),
            {"GITEA_TOKEN": "abc", "OTHER": "1"},
        )

    def test_export_quotes_and_comments(self):
        parsed = env.parse_env_text(
            "# comment\nexport GITEA_TOKEN='quoted value'\n"
            "RUN_ENV=\"dev\"\n\nnot a pair line\n"
        )
        self.assertEqual(parsed["GITEA_TOKEN"], "quoted value")
        self.assertEqual(parsed["RUN_ENV"], "dev")
        self.assertNotIn("not a pair line", "".join(parsed))

    def test_variable_expansion_with_default(self):
        parsed = env.parse_env_text(
            "BASE=https://gitea.test\nAPI=${BASE}/api/v1\nEXTRA=${MISSING:-fallback}\n"
        )
        self.assertEqual(parsed["API"], "https://gitea.test/api/v1")
        self.assertEqual(parsed["EXTRA"], "fallback")

    def test_expansion_can_see_the_process_environment(self):
        parsed = env.parse_env_text("API=${HOST}/api/v1", {"HOST": "git.example"})
        self.assertEqual(parsed["API"], "git.example/api/v1")


class DeriveRemoteTests(unittest.TestCase):
    def test_https_url(self):
        self.assertEqual(
            env.derive_remote("https://git.ehr.gov.mo/ehr/WeightManagement-frontend.git"),
            ("git.ehr.gov.mo", "ehr/WeightManagement-frontend"),
        )

    def test_ssh_url_with_port(self):
        self.assertEqual(
            env.derive_remote("ssh://git@git.tokyofy.top:33022/Atom/rockbot.git"),
            ("git.tokyofy.top", "Atom/rockbot"),
        )

    def test_scp_like_form(self):
        self.assertEqual(
            env.derive_remote("git@git.example:owner/repo.git"),
            ("git.example", "owner/repo"),
        )

    def test_no_remote_configured(self):
        self.assertEqual(env.derive_remote(""), ("", ""))


class LoadSettingsTests(unittest.TestCase):
    def write_env(self, directory: Path, text: str) -> Path:
        path = directory / ".env"
        path.write_text(text, encoding="utf-8")
        return path

    def test_dotenv_supplies_token_and_derivation(self):
        with tempfile.TemporaryDirectory() as tmp:
            cwd = Path(tmp)
            self.write_env(cwd, "GITEA_TOKEN=from-file\n")
            settings = env.load_settings(
                api="https://gitea.test/api/v1", repo="Atom/demo",
                cwd=cwd, root=cwd, process_env={}
            )
            self.assertEqual(settings.token, "from-file")
            self.assertEqual(settings.repo, "Atom/demo")
            self.assertIn(str(cwd / ".env"), settings.env_files)

    def test_process_environment_beats_the_dotenv_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            cwd = Path(tmp)
            self.write_env(cwd, "GITEA_TOKEN=stale\n")
            settings = env.load_settings(
                api="https://gitea.test/api/v1", repo="Atom/demo",
                cwd=cwd, root=cwd, process_env={"GITEA_TOKEN": "exported"}
            )
            self.assertEqual(settings.token, "exported")

    def test_explicit_api_and_repo_overrides_are_used(self):
        with tempfile.TemporaryDirectory() as tmp:
            cwd = Path(tmp)
            settings = env.load_settings(
                repo="Atom/other", api="https://gitea.test:8443", cwd=cwd, root=cwd,
                process_env={"GITEA_TOKEN": "t"},
            )
            self.assertEqual(settings.repo, "Atom/other")
            self.assertEqual(settings.api_base, "https://gitea.test:8443/api/v1")

    def test_env_file_override_replaces_the_search(self):
        with tempfile.TemporaryDirectory() as tmp:
            cwd = Path(tmp)
            secrets = cwd / "secrets"
            secrets.mkdir()
            custom = self.write_env(secrets, "GITEA_TOKEN=custom\n")
            self.write_env(cwd, "GITEA_TOKEN=ignore-me\n")
            settings = env.load_settings(
                env_file=str(custom), api="https://gitea.test/api/v1",
                repo="Atom/demo", cwd=cwd, root=cwd, process_env={}
            )
            self.assertEqual(settings.token, "custom")

    def test_require_names_the_missing_token(self):
        settings = env.Settings(api_base="https://gitea.test/api/v1",
                               repo="Atom/demo", token="")
        with self.assertRaises(env.ConfigError) as caught:
            settings.require()
        message = str(caught.exception)
        self.assertIn("GITEA_TOKEN", message)
        self.assertNotIn("secret", message.lower())

    def test_require_names_a_bad_owner_repo(self):
        settings = env.Settings(api_base="https://gitea.test/api/v1", repo="demo",
                               token="t")
        with self.assertRaises(env.ConfigError) as caught:
            settings.require()
        self.assertIn("--repo owner/repo", str(caught.exception))

    def test_summary_never_leaks_the_token(self):
        settings = env.Settings(api_base="https://gitea.test/api/v1",
                               repo="Atom/demo", token="super-secret-token")
        text = str(settings.summary())
        self.assertNotIn("super-secret-token", text)
        self.assertEqual(settings.summary()["token_chars"], 18)

    def test_repo_and_issue_urls_drop_the_api_prefix(self):
        settings = env.Settings(api_base="https://gitea.test/api/v1",
                               repo="Atom/demo", token="t")
        self.assertEqual(settings.repo_url, "https://gitea.test")
        self.assertEqual(settings.issues_url(687),
                         "https://gitea.test/issues/687")


if __name__ == "__main__":
    unittest.main()
