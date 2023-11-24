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
        return reply["choices"][0]["message"]

    async def _post(self, data):
        # print("Sending the following request to openai:", data)
        async with aiohttp.ClientSession(headers=self.headers) as s:
            async with s.post(self.chat_completions_api, json=data) as response:
                r = await response.json()
                return r

    @defJson()
    def _parse_message(self, message):
        return message["choices"][0]["message"]["content"]

    async def submit(self, rid, message) -> str:
        h = OpenAi.histories.get(rid, [])
        m = self._compose_message(message, h)
        r = await self._post(m)
        if t := self._parse_message(r):
            OpenAi.histories[rid] = [*m["messages"], self._parse_reply(r)]
            return t.strip()
        else:
            return str(r)


class chatBot(chatABC):
    def __init__(self, openai=OpenAi()):
        self.openai = openai

    async def chat(self, bot):
        if (
            bot.rid in OpenAi.histories
            and len(OpenAi.histories[bot.rid]) > conf.max_history
        ):
            # Remove the oldest message
            OpenAi.histories[bot.rid].pop(0)
        asyncio.create_task(self._query(bot, bot.rid, bot.msg))

    async def _query(self, bot, rid, msg) -> None:
        await bot.reply(await self.openai.submit(rid, msg))
