import asyncio
from bot.openai import OpenAi


class Test:
    chat_prompt = {
        "draw": "Please use the draw function to draw a picture of a cat.",
        "show": "Please show me the picture you just drew.",
        "gpt4": "Hi there! Are you GPT-4?",
        "chat": "The following is a conversation with an AI assistant. The assistant is helpful, creative, clever, and very friendly.",
        "news": "Tell me the latest news in Technology.",
        "weather": "What's the weather like today in Tokyo?",
    }

    def __init__(self):
        self.openai = OpenAi()

    async def testAll(self):
        """
        await self.testChat()
        await self.testDraw()
        await self.testChat("draw")
        await self.testChat("show")
        await self.testChat("gpt4")
        await self.testChat("chat")
        """
        await self.testChat("news")
        await self.testChat("gpt4")
        await self.testChat("chat")
        await self.testChat("weather")

        print("All test passed")

    async def testDraw(self):
        r = await self.openai.draw("Random image")
        assert "created" in r and "data" in r, "Failed to draw image"
        print("Image created: ", r["data"][0]["url"])
        print("Test Draw passed")

    async def testChatnDraw(self, prompt="gpt4"):
        r = await self.openai.submit("test", Test.chat_prompt["draw"])
        assert r
        print(r)
        print("Test Chat passed")

    async def testChat(self, prompt):
        r = await self.openai.submit("test", Test.chat_prompt[prompt])
        assert r
        print(r)
        print("Test Chat passed")


if __name__ == "__main__":
    test = Test()
    # Run all tests using asyncio
    loop = asyncio.get_event_loop()
    loop.run_until_complete(test.testAll())
    loop.close()
