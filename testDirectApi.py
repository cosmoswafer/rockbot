import asyncio
from bot.openai import ApiClient


class Test(ApiClient):
    template_messsages = {
        "user1": {"role": "user", "content": "Hello"},
        "asst1": {"role": "assistant", "content": "I'm ChatGPT!"},
        "user2": {"role": "user", "content": "How are you?"},
        "asst2": {"role": "assistant", "content": "I'm fine, thanks!"},
        "tools": {"role": "tool", "content": "{'url': 'https://www.google.com'}"},
        "user3": {
            "role": "user",
            "content": "What's the results in the above tool call result?",
        },
    }

    def _composePlayload(self, messages: list) -> dict:
        """
        Compose the full playload for the API call with the list of messages
        Each message is a dict with the following keys:
        - role: str
        - content: str
        - tool_call_id: str (optional for function calls)
        """
        return self.postdata | {"messsages": messages}

    async def testAll(self):
        await self.testPost()

    async def testPost(self):
        post_data = self._composePlayload([self.template_messsages["user2"]])
        r = await self.apiPost(self.chat_completions_api, post_data)
        print(r)
        print("test passed")


if __name__ == "__main__":
    loop = asyncio.get_event_loop()
    loop.run_until_complete(Test().testAll())
    loop.close()
