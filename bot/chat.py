#!/usr/bin/env python

import asyncio
from config import openai as conf
from bot.openai import OpenAi


class chatBot:
    def __init__(self, openai=OpenAi()):
        self.openai = openai

    async def chat(self, bot):
        if conf.debug:
            print(f"chatBot incoming message: [{bot.rid}]{bot.msg}")
        asyncio.create_task(self._query(bot, bot.rid, bot.msg))

    async def _query(self, bot, rid, msg) -> None:
        await bot.reply(await self.openai.submit(rid, msg))
