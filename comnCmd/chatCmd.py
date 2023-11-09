#!/usr/bin/env python

import asyncio
import aiohttp
from cmdAbc import cmd


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
    chat_completions_api = "https://agent.evanora.top/v1/chat/completions"
    headers = {
        "Content-Type": "application/json",
        "Authorization": "Bearer sk-REPLACED",
    }
    postdata = {"model": "gpt-4-32k", "messages": []}
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


class chatCmd(cmd):
    def __init__(self, parser, openai=OpenAi()):
        self.parser = parser
        self.parser.add_argument(
            "-r", "--clear-history", help="Clear the chat history", action="store_true"
        )
        self.parser.add_argument("keywords", help="Query keywords", type=str, nargs="*")
        self.parser.set_defaults(func=self.update)

        self.openai = openai

    async def update(self, bot):
        if bot.args.clear_history:
            OpenAi.histories[bot.rid] = []
            await bot.reply("Clear and start a new chat")
        # await bot.reply(await self._query(bot.rid, bot.args.keywords))
        asyncio.create_task(self._query(bot, bot.rid, bot.args.keywords))

    async def _query(self, bot, rid, keywords):
        # return await self.openai.submit(rid, " ".join(keywords))
        await bot.reply(await self.openai.submit(rid, " ".join(keywords)))
