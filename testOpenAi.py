import asyncio
from bot.openai import OpenAi


class Test:
    def __init__(self):
        self.openai = OpenAi()

    async def testAll(self):
        # await self.testChat()
        await self.testDraw()

    async def testDraw(self):
        r = await self.openai.draw("Random image")
        assert "created" in r and "data" in r, "Failed to draw image"
        print("Image created: ", r["data"][0]["url"])
        print("Test Draw passed")

    async def testChat(self):
        r = await self.openai.submit("test", "Hi there! Are you GPT-4?")
        print(r)
        print("Test Chat passed")


if __name__ == "__main__":
    test = Test()
    # Run all tests using asyncio
    loop = asyncio.get_event_loop()
    loop.run_until_complete(test.testAll())
    loop.close()
