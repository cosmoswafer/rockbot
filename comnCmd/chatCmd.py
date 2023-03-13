#!/usr/bin/env python

import asyncio
import aiohttp
from cmdAbc import cmd


class OpenAi:
    chat_completions_api = "https://api.openai.com/v1/chat/completions"
    headers = {
        "Content-Type": "application/json",
        "Authorization": "Bearer sk-REPLACED",
    }
    postdata = {"model": "gpt-3.5-turbo", "messages": []}
    msg_template = {"role": "user", "content": ""}

    def _new_message(self, message):
        pd = {**self.postdata}
        pd["messages"].append({**self.msg_template, "content": message})

    async def _post(self, data):
        async with aiohttp.ClientSession(headers=self.headers) as s:
            async with s.post(self.chat_completions_api, json=data) as response:
                r = await response.json()
                return r

    def _parse_message(self, message: dict):
        return message["choices"][0]["message"]["content"]

    async def submit(self, message):
        return self._parse_message(await self._post(self._new_message(message)))


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
            await bot.reply("Clear and start a new chat")
        await bot.reply(await self._query(bot.args.keywords))

    async def _query(self, keywords):
        return await self.openai.submit(" ".join(keywords))
