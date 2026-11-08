"""Tests for the pure UTC/time-window helpers."""

import unittest

from gitea_bots.clock import shift_iso, timestamp_in_window, to_epoch


class ClockTests(unittest.TestCase):
    def test_to_epoch_accepts_z_and_offset(self):
        self.assertEqual(
            to_epoch("2026-11-08T00:00:00Z"),
            to_epoch("2026-11-08T00:00:00+00:00"),
        )

    def test_to_epoch_invalid_returns_none(self):
        self.assertIsNone(to_epoch(""))
        self.assertIsNone(to_epoch("not-a-date"))

    def test_shift_iso(self):
        self.assertEqual(
            shift_iso("2026-11-08T00:02:00Z", -120),
            "2026-11-08T00:00:00Z",
        )

    def test_shift_iso_invalid_returns_none(self):
        self.assertIsNone(shift_iso("garbage", -120))

    def test_window_no_start_means_nothing_in_window(self):
        self.assertFalse(timestamp_in_window("2026-11-08T00:00:00Z", "", 0))

    def test_window_zero_epoch_accepts_everything(self):
        self.assertTrue(timestamp_in_window("anything", "1970-01-01T00:00:00Z", 0))

    def test_window_unparseable_timestamp_fails_open(self):
        self.assertTrue(timestamp_in_window("garbage", "x", 1_000_000))

    def test_window_epoch_comparison(self):
        epoch = to_epoch("2026-11-08T00:01:00Z")
        self.assertTrue(
            timestamp_in_window("2026-11-08T00:02:00Z", "x", epoch)
        )
        self.assertFalse(
            timestamp_in_window("2026-11-08T00:00:00Z", "x", epoch)
        )


if __name__ == "__main__":
    unittest.main()
