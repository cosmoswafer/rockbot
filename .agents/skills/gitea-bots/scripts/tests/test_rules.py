"""Tests for the pure workflow decision core (no I/O)."""

import unittest

from gitea_bots.rules import (
    STAGE2_MARKER,
    STAGE3_MARKER,
    auto_plan_issues_in_batch,
    body_mention_digest,
    body_mention_paragraphs,
    classify_comment,
    comments_have_stage2_marker,
    comments_have_stage3_marker,
    extract_tasks,
    has_bot_mention,
    has_summary,
    has_tasks,
    human_reply_blocks,
    is_bot_output,
    is_directed_mention,
    strip_code_spans,
    strip_one_trailing_newline,
    strip_tasks_section,
    trim_trailing_blank_lines,
)


class DirectedMentionTests(unittest.TestCase):
    def test_human_mention(self):
        self.assertTrue(is_directed_mention("alice", "@testbot status?", "testbot"))

    def test_case_insensitive(self):
        self.assertTrue(is_directed_mention("alice", "ping @TestBot hi", "testbot"))

    def test_word_bounded(self):
        self.assertFalse(is_directed_mention("alice", "@testbot2 hi", "testbot"))
        self.assertFalse(is_directed_mention("alice", "@testbot-foo hi", "testbot"))

    def test_shared_login_self_authored_mention_counts(self):
        # #136: bot and human share one account, so an at-mention is a human
        # reply even when the author equals the token user.
        self.assertTrue(is_directed_mention("testbot", "@testbot hi", "testbot"))

    def test_dedicated_bot_guard_restored(self):
        self.assertFalse(is_directed_mention("testbot", "@testbot hi", "testbot",
                                            shared_login=False))
        self.assertTrue(is_directed_mention("alice", "@testbot hi", "testbot",
                                           shared_login=False))

    def test_plain_self_authored_comment_is_dropped(self):
        self.assertFalse(is_directed_mention("testbot", "just a note", "testbot"))

    def test_no_bot_identity(self):
        self.assertFalse(is_directed_mention("alice", "@testbot hi", ""))


class SharedLoginHelperTests(unittest.TestCase):
    def test_strip_code_spans(self):
        body = "prose `@testbot` inline\n```\n@testbot fenced\n```\n    @testbot indented\n@testbot real"
        cleaned = strip_code_spans(body)
        self.assertNotIn("fenced", cleaned)
        self.assertNotIn("indented", cleaned)
        self.assertIn("@testbot real", cleaned)

    def test_has_bot_mention_outside_code(self):
        self.assertTrue(has_bot_mention("hello @testbot", "testbot"))
        self.assertFalse(has_bot_mention("hello", "testbot"))
        self.assertFalse(has_bot_mention("hello @testbot", ""))

    def test_quoted_mentions_ignored(self):
        self.assertFalse(has_bot_mention("`@testbot`", "testbot"))
        self.assertFalse(has_bot_mention("```\n@testbot\n```", "testbot"))
        self.assertFalse(has_bot_mention("    @testbot", "testbot"))
        self.assertTrue(has_bot_mention("`@testbot` but @testbot here", "testbot"))

    def test_is_bot_output(self):
        self.assertTrue(is_bot_output(f"plan\n{STAGE2_MARKER}"))
        self.assertTrue(is_bot_output(f"done\n{STAGE3_MARKER}"))
        self.assertFalse(is_bot_output("human text"))

    def test_bot_output_exempt_from_directed(self):
        self.assertFalse(is_directed_mention(
            "testbot", f"@testbot\n{STAGE2_MARKER}", "testbot"))


class ClassifyTests(unittest.TestCase):
    def test_all_comments_wins(self):
        self.assertEqual(
            classify_comment("alice", "hello", self_login="testbot",
                             all_comments=True),
            "NEW COMMENT",
        )

    def test_directed_mention(self):
        self.assertEqual(
            classify_comment("alice", "@testbot hello", self_login="testbot",
                             all_comments=False),
            "DIRECTED COMMENT",
        )

    def test_plain_chat_ignored(self):
        self.assertIsNone(
            classify_comment("alice", "just chatting", self_login="testbot",
                             all_comments=False)
        )

    def test_command_like_text_without_mention_is_ignored(self):
        # #134: the skill defines no command vocabulary. Wording that used to
        # be a stage trigger is only a request when a human at-mentions the bot.
        for body in ["/plan", "/implement", "/investigate",
                     "execute dev flow", "implement the plan",
                     "plan the implementation"]:
            with self.subTest(body=body):
                self.assertIsNone(
                    classify_comment("alice", body, self_login="testbot",
                                     all_comments=False)
                )

    def test_shared_login_self_authored_directed(self):
        self.assertEqual(
            classify_comment("testbot", "@testbot status?", self_login="testbot",
                             all_comments=False),
            "DIRECTED COMMENT",
        )

    def test_dedicated_bot_self_authored_dropped(self):
        self.assertIsNone(
            classify_comment("testbot", "@testbot hi", self_login="testbot",
                             all_comments=False, shared_login=False)
        )

    def test_bot_output_not_directed(self):
        self.assertIsNone(
            classify_comment("testbot", f"@testbot\n{STAGE2_MARKER}",
                             self_login="testbot", all_comments=False)
        )


class MarkerAndSectionTests(unittest.TestCase):
    def test_stage_markers(self):
        comments = [{"body": "x"}, {"body": f"plan\n{STAGE2_MARKER}"}]
        self.assertTrue(comments_have_stage2_marker(comments))
        self.assertFalse(comments_have_stage3_marker(comments))
        self.assertTrue(
            comments_have_stage3_marker([{"body": f"done {STAGE3_MARKER}"}])
        )

    def test_body_sections(self):
        body = "## Summary\n\nHi.\n\n## Tasks\n\n- [ ] x\n"
        self.assertTrue(has_summary(body))
        self.assertTrue(has_tasks(body))
        self.assertFalse(has_tasks("## Summary only\n"))
        # trailing whitespace on the heading is accepted
        self.assertTrue(has_tasks("## Tasks   \n\n- [ ] x\n"))


class BodyMentionTests(unittest.TestCase):
    """#185: an at-mention in the issue body is the same request as a comment."""

    def test_returns_only_mention_paragraphs(self):
        body = (
            "## Summary\n\n"
            "Something harmless.\n\n"
            "@testbot implement this, please\n\n"
            "Unrelated trailing note.\n"
        )
        self.assertEqual(
            body_mention_paragraphs("alice", body, "testbot"),
            ["@testbot implement this, please"],
        )

    def test_multi_line_paragraph_is_kept_whole(self):
        body = "intro\n\n@testbot do the thing\nand this too\n\noutro\n"
        self.assertEqual(
            body_mention_paragraphs("alice", body, "testbot"),
            ["@testbot do the thing\nand this too"],
        )

    def test_quoted_and_code_mentions_are_examples(self):
        body = (
            "`@testbot` inline\n\n"
            "```\n@testbot fenced\n```\n\n"
            "    @testbot indented\n\n"
            "no real mention\n"
        )
        self.assertEqual(body_mention_paragraphs("alice", body, "testbot"), [])

    def test_bot_output_is_exempt(self):
        body = f"@testbot plan follows\n\n{STAGE2_MARKER}\n"
        self.assertEqual(body_mention_paragraphs("alice", body, "testbot"), [])

    def test_dedicated_bot_ignores_self_authored_body(self):
        body = "@testbot hi\n"
        self.assertEqual(
            body_mention_paragraphs("testbot", body, "testbot",
                                    shared_login=False),
            [],
        )
        self.assertEqual(
            body_mention_paragraphs("alice", body, "testbot",
                                    shared_login=False),
            ["@testbot hi"],
        )

    def test_unknown_identity_fails_closed(self):
        self.assertEqual(body_mention_paragraphs("alice", "@testbot hi", ""), [])

    def test_digest_is_stable_and_ignores_unrelated_edits(self):
        first = body_mention_paragraphs(
            "alice", "intro\n\n@testbot implement\n\noutro\n", "testbot"
        )
        edited_outro = body_mention_paragraphs(
            "alice", "intro changed\n\n@testbot implement\n\noutro changed\n",
            "testbot",
        )
        changed_mention = body_mention_paragraphs(
            "alice", "intro\n\n@testbot implement differently\n\noutro\n",
            "testbot",
        )
        self.assertEqual(body_mention_digest(first), body_mention_digest(edited_outro))
        self.assertNotEqual(
            body_mention_digest(first), body_mention_digest(changed_mention)
        )
        self.assertEqual(body_mention_digest([]), "")


class HumanReplyBlockTests(unittest.TestCase):
    """#131/#136/#134: per-issue blocks keyed on a DIRECTED human reply."""

    HUMAN = (
        "2026-11-08T05:07:56Z DIRECTED COMMENT on #130 (comment 2716) by alice "
        "— https://gitea.test/x/issues/130\n"
        "    | @testbot please implement this\n"
    )
    SELF = (
        "2026-11-08T05:07:56Z DIRECTED COMMENT on #130 (comment 2716) by testbot "
        "— https://gitea.test/x/issues/130\n"
        "    | please implement this\n"
    )
    SELF_MENTION = (
        "2026-11-08T05:07:56Z DIRECTED COMMENT on #130 (comment 2716) by testbot "
        "— https://gitea.test/x/issues/130\n    | @testbot /implement\n"
    )
    SELF_FENCED = (
        "2026-11-08T05:07:56Z DIRECTED COMMENT on #130 (comment 2716) by testbot "
        "— https://gitea.test/x/issues/130\n    | `@testbot` /implement\n"
    )
    SELF_BOT_OUTPUT = (
        "2026-11-08T05:07:56Z DIRECTED COMMENT on #130 (comment 2716) by testbot "
        "— https://gitea.test/x/issues/130\n    | @testbot /implement\n"
        f"    | {STAGE2_MARKER}\n"
    )

    def test_human_reply_detected(self):
        blocks = human_reply_blocks(self.HUMAN, "testbot")
        self.assertEqual(sorted(blocks), [130])
        self.assertIn("please implement this", blocks[130])

    def test_plain_self_authored_comment_is_not_a_reply(self):
        # A comment from the token user with no at-mention stays
        # surfaced-but-non-authorizing (loop avoidance).
        self.assertEqual(human_reply_blocks(self.SELF, "testbot"), {})

    def test_shared_login_mention_is_a_human_reply(self):
        # #136: an at-mention makes it a human reply even from the shared
        # account.
        self.assertEqual(
            sorted(human_reply_blocks(self.SELF_MENTION, "testbot")), [130]
        )

    def test_dedicated_bot_mention_is_not_human(self):
        self.assertEqual(
            human_reply_blocks(self.SELF_MENTION, "testbot", shared_login=False),
            {},
        )

    def test_fenced_mention_is_not_human(self):
        self.assertEqual(human_reply_blocks(self.SELF_FENCED, "testbot"), {})

    def test_bot_output_body_is_exempt(self):
        self.assertEqual(human_reply_blocks(self.SELF_BOT_OUTPUT, "testbot"), {})

    def test_catchup_variant(self):
        text = (
            "2026-11-08T05:07:56Z DIRECTED COMMENT (catch-up) on #5 (comment 9) "
            "by alice — https://gitea.test/x/issues/5\n    | @testbot status?\n"
        )
        self.assertEqual(sorted(human_reply_blocks(text, "testbot")), [5])

    def test_unknown_bot_identity_fails_closed(self):
        self.assertEqual(human_reply_blocks(self.HUMAN, ""), {})
        self.assertEqual(human_reply_blocks(self.SELF_MENTION, ""), {})

    def test_other_events_ignored(self):
        text = (
            "2026-11-08T05:07:56Z NEW ISSUE #3 [open] \"X\" by alice — url\n"
            "2026-11-08T05:07:56Z AUTO PLAN #4 \"Y\" by alice — url\n"
            "2026-11-08T05:07:56Z NEW COMMENT on #5 (comment 6) by alice — url\n"
            "    | @testbot status?\n"
        )
        self.assertEqual(human_reply_blocks(text, "testbot"), {})

    def test_human_reply_blocks_split_per_issue(self):
        text = (
            "2026-11-08T05:07:56Z DIRECTED COMMENT on #130 (comment 1) by alice "
            "— https://gitea.test/x/issues/130\n    | @testbot do the thing\n"
            "2026-11-08T05:07:57Z DIRECTED COMMENT on #131 (comment 2) by bob "
            "— https://gitea.test/x/issues/131\n    | @testbot check this\n"
        )
        blocks = human_reply_blocks(text, "testbot")
        self.assertEqual(sorted(blocks), [130, 131])
        self.assertIn("#130", blocks[130])
        self.assertIn("do the thing", blocks[130])
        self.assertNotIn("#131", blocks[130])
        self.assertIn("#131", blocks[131])
        self.assertIn("check this", blocks[131])

    def test_reply_blocks_skip_plain_self_and_quoted(self):
        self.assertEqual(human_reply_blocks(self.SELF, "testbot"), {})
        self.assertEqual(human_reply_blocks(self.SELF_FENCED, "testbot"), {})
        self.assertEqual(human_reply_blocks(self.SELF_BOT_OUTPUT, "testbot"), {})
        self.assertEqual(human_reply_blocks(self.HUMAN, ""), {})

    def test_body_mention_event_is_a_human_reply(self):
        text = (
            "2026-11-08T05:07:56Z DIRECTED COMMENT on #130 (issue body) by "
            "alice — https://gitea.test/x/issues/130\n"
            "    | @testbot please implement this\n"
        )
        blocks = human_reply_blocks(text, "testbot")
        self.assertEqual(sorted(blocks), [130])
        self.assertIn("please implement this", blocks[130])

    def test_body_mention_variant_is_parsed_with_catchup(self):
        text = (
            "2026-11-08T05:07:56Z DIRECTED COMMENT (catch-up) on #5 "
            "(issue body) by testbot — https://gitea.test/x/issues/5\n"
            "    | @testbot status?\n"
        )
        self.assertEqual(sorted(human_reply_blocks(text, "testbot")), [5])

    def test_multiple_replies_for_one_issue_are_concatenated(self):
        text = (
            "2026-11-08T05:07:56Z DIRECTED COMMENT on #130 (comment 1) by alice "
            "— https://gitea.test/x/issues/130\n    | @testbot hello\n"
            "2026-11-08T05:07:57Z DIRECTED COMMENT on #130 (comment 2) by bob "
            "— https://gitea.test/x/issues/130\n    | @testbot more detail\n"
        )
        blocks = human_reply_blocks(text, "testbot")
        self.assertEqual(sorted(blocks), [130])
        self.assertIn("hello", blocks[130])
        self.assertIn("more detail", blocks[130])


class BatchAutoPlanTests(unittest.TestCase):
    """#140: AUTO PLAN event lines are a second stage-2 authorization shape."""

    def test_real_event_lines_are_parsed(self):
        text = (
            "2026-11-08T07:31:23Z AUTO PLAN #138 \"Use server time\" by saru "
            "— https://gitea.test/x/issues/138\n"
            "2026-11-08T07:31:24Z AUTO PLAN #140 \"Another\" by saru "
            "— https://gitea.test/x/issues/140\n"
        )
        self.assertEqual(auto_plan_issues_in_batch(text), [138, 140])

    def test_duplicates_are_deduped_and_sorted(self):
        text = (
            "2026-11-08T07:31:24Z AUTO PLAN #9 \"B\" by saru — url\n"
            "2026-11-08T07:31:25Z AUTO PLAN #3 \"A\" by saru — url\n"
            "2026-11-08T07:31:26Z AUTO PLAN #9 \"B\" by saru — url\n"
        )
        self.assertEqual(auto_plan_issues_in_batch(text), [3, 9])

    def test_indented_body_line_does_not_authorize(self):
        text = (
            "2026-11-08T07:31:23Z NEW COMMENT on #1 (comment 2) by alice — url\n"
            "    | AUTO PLAN #138 was emitted earlier\n"
        )
        self.assertEqual(auto_plan_issues_in_batch(text), [])

    def test_title_mention_and_other_tags_do_not_authorize(self):
        text = (
            "2026-11-08T07:31:23Z NEW ISSUE #5 [open] \"AUTO PLAN is broken\" "
            "by alice — url\n"
            "AUTO PLAN #6 \"No timestamp\" by saru — url\n"
        )
        self.assertEqual(auto_plan_issues_in_batch(text), [])

    def test_empty_text(self):
        self.assertEqual(auto_plan_issues_in_batch(""), [])
        self.assertEqual(auto_plan_issues_in_batch(None), [])


class TaskHelperTests(unittest.TestCase):
    PLAN = (
        "## Findings\n\nstuff\n\n"
        "## Tasks\n\n- [ ] one\n- [x] two\n\n"
        "## Code sketch\n\n- [ ] ignore me\n"
    )

    def test_extract_tasks_only_from_section(self):
        self.assertEqual(extract_tasks(self.PLAN), ["- [ ] one", "- [x] two"])

    def test_no_checklist(self):
        self.assertEqual(extract_tasks("## Tasks\n\nnothing here\n"), [])

    def test_strip_tasks_section(self):
        body = "## Summary\n\nHi.\n\n## Tasks\n\n- [ ] x\n\n## Related\n\nfoo\n"
        stripped = strip_tasks_section(body)
        self.assertNotIn("- [ ] x", "\n".join(stripped))
        self.assertIn("## Related", "\n".join(stripped))
        self.assertIn("## Summary", "\n".join(stripped))

    def test_trim_trailing_blank_lines(self):
        self.assertEqual(
            trim_trailing_blank_lines(["a", "", "  ", "b", "", ""]), ["a", "", "  ", "b"]
        )

    def test_strip_one_trailing_newline(self):
        self.assertEqual(strip_one_trailing_newline("a\n"), "a")
        self.assertEqual(strip_one_trailing_newline("a"), "a")
        self.assertEqual(strip_one_trailing_newline("a\n\n"), "a\n")


if __name__ == "__main__":
    unittest.main()
