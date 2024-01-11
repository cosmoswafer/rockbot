import unittest
from unittest.mock import MagicMock, AsyncMock
from bot.openai import OpenAi


class TestOpenAi(unittest.TestCase):
    def setUp(self):
        self.openai = OpenAi()

    def test_compose_message(self):
        message = "Test message"
        history = [{"content": "Previous message"}]
        expected = {
            "messages": [{"content": "Previous message"}, {"content": "Test message"}]
        }
        result = self.openai._compose_message(message, history)
        self.assertEqual(result, expected)

    def test_compose_reply(self):
        reply = "Test reply"
        expected = None  # Replace with expected result
        result = self.openai._compose_reply(reply)
        self.assertEqual(result, expected)

    # Add more test methods for other functions in the OpenAi class


if __name__ == "__main__":
    unittest.main()
