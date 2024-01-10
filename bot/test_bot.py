import unittest
from unittest.mock import MagicMock, AsyncMock
from bot import bot


class TestBot(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        self.rocket = AsyncMock()
        self.msg = "Test message"
        self.rid = "123"
        self.test_bot = bot(self.rocket, self.msg, self.rid)

    async def test_reply(self):
        msg = "Reply message"
        await self.test_bot.reply(msg)
        self.rocket.sendMsg.assert_called_once_with(self.rid, msg)

    async def test_replyQ(self):
        msg = "Question message"
        expected_msg_q = f"```\n{msg}\n```"
        await self.test_bot.replyQ(msg)
        self.rocket.sendMsg.assert_called_once_with(self.rid, expected_msg_q)

    async def test_Bot_man(self):
        b = bot(self.rocket, "   !help", self.rid)
        assert b.msg == "!help"


if __name__ == "__main__":
    unittest.main()
