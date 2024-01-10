import unittest
from unittest.mock import MagicMock, AsyncMock, patch
from bot.chat import chatBot
import config


class TestChatBot(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        self.openai = MagicMock()
        self.bot = AsyncMock()
        """
        with patch("bot.chat.config", new=MagicMock()):
            self.chat_bot = chatBot(self.openai)
        """
        self.chat_bot = chatBot(self.openai)

    async def test_clear(self):
        self.openai.histories = {"123": ["message1", "message2"]}
        await self.chat_bot.clear(self.bot)
        self.assertEqual(self.openai.histories["123"], [])
        self.bot.reply.assert_called_once_with("History cleared")

    async def test_help(self):
        self.chat_bot.commands = {
            "help": {"fnc": self.chat_bot.help, "desc": "Display available commands"},
            "clear": {"fnc": self.chat_bot.clear, "desc": "Clear chat history"},
        }
        expected_commands = (
            "!help: Display available commands\n!clear: Clear chat history"
        )
        await self.chat_bot.help(self.bot)
        self.bot.reply.assert_called_once_with(f"Commands:\n{expected_commands}")

    async def test_chat_command(self):
        self.chat_bot.commands = {
            "help": {"fnc": self.chat_bot.help, "desc": "Display available commands"},
            "clear": {"fnc": self.chat_bot.clear, "desc": "Clear chat history"},
        }
        self.bot.msg = "!help"
        await self.chat_bot.chat(self.bot)
        self.bot.reply.assert_called_once_with("Display available commands")

    """
        async def test_chat_query(self):
            self.chat_bot.commands = {
                "help": {"fnc": self.chat_bot.help, "desc": "Display available commands"},
                "clear": {"fnc": self.chat_bot.clear, "desc": "Clear chat history"},
            }
            self.bot.msg = "Some message"
            self.openai.submit.return_value = "Response from OpenAI"
            await self.chat_bot.chat(self.bot)
            self.openai.submit.assert_called_once_with(self.bot.rid, self.bot.msg)
            self.bot.reply.assert_called_once_with("Response from OpenAI")
    """


if __name__ == "__main__":
    unittest.main()
