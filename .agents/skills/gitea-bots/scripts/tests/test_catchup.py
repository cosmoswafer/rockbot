"""CLI surface for the separate catch-up command."""

import io
import unittest
from contextlib import redirect_stdout
from unittest.mock import patch

from gitea_bots.catchup import main_catchup


class CatchupCliTests(unittest.TestCase):
    def test_without_arguments_prints_help(self):
        out = io.StringIO()
        with redirect_stdout(out):
            self.assertEqual(main_catchup([]), 0)
        self.assertIn("catch-up.py", out.getvalue())
        self.assertIn("replay", out.getvalue())

    def test_rejects_an_unknown_mode(self):
        with patch("sys.stderr", new_callable=io.StringIO):
            with self.assertRaises(SystemExit) as ctx:
                main_catchup(["bogus"])
        self.assertEqual(ctx.exception.code, 2)


if __name__ == "__main__":
    unittest.main()