import asyncio
import aiohttp
from urllib.parse import urljoin
from util.config import openai as conf
from util.decorators import defJson, retryA


class OpenAi:
    chat_completions_api = urljoin(conf.url.base, conf.url.chat)
    draw_images_api = urljoin(conf.url.base, conf.url.draw)
    headers = {
        "Content-Type": "application/json",
        "Authorization": f"Bearer {conf.key}",
    }
    postdata = {"model": conf.model, "messages": []}
    msg_template = {"role": "user", "content": ""}
    rep_template = {"role": "assistant", "content": ""}
    draw_data = {
        "prompt": "a photo of a happy corgi puppy sitting and facing forward, studio light, longshot",
        "n": 1,
        "size": "1024x1024",
    }

    histories = {}
    # TODO implemente the presistence history class

    def _compose_message(self, message, history=[]):
        pd = {**self.postdata}
        pd["messages"] = [*history, {**self.msg_template, "content": message}]
        return pd

    def _compose_reply(self, reply):
        pass

    @defJson()
    def _parse_reply(self, reply):
        # return reply["choices"][0]["message"]
        reply_content = reply["choices"][0]["message"]
        if reply_content["role"] != "assistant":
            # Patch the role
            reply_content["role"] = "assistant"
        return reply_content

    @retryA(5)
    async def _post(self, url, data):
        # print("Sending the following request to openai:", data)
        async with aiohttp.ClientSession(headers=self.headers) as s:
            async with s.post(url, json=data) as response:
                if response.status != 200:
                    if conf.debug:
                        print(f"Response body: {await response.text()}")
                    raise Exception(
                        f"Failed to post data to openai, status code: {response.status}"
                    )
                else:
                    return await response.json()

    @defJson()
    def _parse_message(self, message):
        return message["choices"][0]["message"]["content"]

    async def _cleanup_history(self, user_id):
        # Check maximum history size
        if (
            user_id in OpenAi.histories
            and len(OpenAi.histories[user_id]) > conf.max_history_size
        ):
            if conf.debug:
                print("chatBot: Removing the old messages")
            # Strip the oldest message and keek the latest ten messages
            OpenAi.histories[user_id] = OpenAi.histories[user_id][
                -1 * conf.max_history_size :
            ]

        # Check maximum text length
        if user_id in OpenAi.histories:
            while (
                len("".join([m["content"] for m in OpenAi.histories[user_id]]))
                > conf.max_text_length
            ):
                if conf.debug:
                    print("chatBot: Removing the oldest message")
                OpenAi.histories[user_id].pop()

    async def draw(self, prompt):
        r = await self._post(self.draw_images_api, self.draw_data | {"prompt": prompt})
        return r

    async def submit(self, user_id, message) -> str:
        await self._cleanup_history(user_id)

        h = OpenAi.histories.get(user_id, [])

        m = self._compose_message(message, h)
        if conf.debug:
            print("OpenAi: Sending the following request to openai:", m)
        r = await self._post(self.chat_completions_api, m)
        if conf.debug:
            print("OpenAi: Received the following response from openai:", r)
        if t := self._parse_message(r):
            OpenAi.histories[user_id] = [*m["messages"], self._parse_reply(r)]
            return t.strip()
        else:
            return str(r)
