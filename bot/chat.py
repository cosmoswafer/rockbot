#!/usr/bin/env python

import asyncio
import aiohttp
from config import openai as conf
from bot.chatAbc import chatABC


def defJson(default_value={}):
    def wrap(f):
        def wrapped_f(*args, **kwargs):
            try:
                return f(*args, **kwargs)
            except (KeyError, IndexError) as e:
                print(
                    f"No such item(s) in json data, return the default value: {default_value}, Exception: {e}"
                )
                return default_value

        return wrapped_f

    return wrap


def retryA(times=3):
    def wrap(f):
        def wrapped_f(*args, **kwargs):
            for i in range(times):
                try:
                    return await f(*args, **kwargs)
                except Exception as e:
                    print(f"Exception: {e}, retrying...")
            raise Exception(f"Failed after {times} times of retrying")

        return wrapped_f

    return wrap


class OpenAi:
    chat_completions_api = conf.url
    headers = {
        "Content-Type": "application/json",
        "Authorization": f"Bearer {conf.key}",
    }
    postdata = {"model": conf.model, "messages": []}
    msg_template = {"role": "user", "content": ""}
    rep_template = {"role": "assistant", "content": ""}

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

    @retryA()
    async def _post(self, data):
        # print("Sending the following request to openai:", data)
        async with aiohttp.ClientSession(headers=self.headers) as s:
            async with s.post(self.chat_completions_api, json=data) as response:
                if response.status != 200:
                    print(f"Response body: {await response.text()}")
                    raise Exception(
                        f"Failed to post data to openai, status code: {response.status}"
                    )
                else:
                    return await response.json()

    @defJson()
    def _parse_message(self, message):
        return message["choices"][0]["message"]["content"]

    async def submit(self, rid, message) -> str:
        if rid in OpenAi.histories and len(OpenAi.histories[rid]) > conf.max_history:
            if conf.debug:
                print("chatBot: Removing the old messages")
            # Strip the oldest message and keek the latest ten messages
            OpenAi.histories[rid] = OpenAi.histories[rid][-1 * conf.max_history :]

        h = OpenAi.histories.get(rid, [])

        m = self._compose_message(message, h)
        if conf.debug:
            print("OpenAi: Sending the following request to openai:", m)
        r = await self._post(m)
        if conf.debug:
            print("OpenAi: Received the following response from openai:", r)
        if t := self._parse_message(r):
            OpenAi.histories[rid] = [*m["messages"], self._parse_reply(r)]
            return t.strip()
        else:
            return str(r)


class chatBot(chatABC):
    def __init__(self, openai=OpenAi()):
        self.openai = openai

    async def chat(self, bot):
        if conf.debug:
            print(f"chatBot incoming message: [{bot.rid}]{bot.msg}")
        asyncio.create_task(self._query(bot, bot.rid, bot.msg))

    async def _query(self, bot, rid, msg) -> None:
        await bot.reply(await self.openai.submit(rid, msg))
