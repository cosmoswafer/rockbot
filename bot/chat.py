#!/usr/bin/env python

import asyncio
from config import openai as conf
from bot.openai import OpenAi


class chatBot:
    def __init__(self, openai=OpenAi()):
        self.openai = openai
        self.commands = {"help": self.help, "clear": self.clear}

    async def clear(self, bot):
        self.openai.histories[bot.room_id] = []
        await bot.reply("History cleared")

    async def help(self, bot):
        await bot.reply(
            "Commands:\n"
            + "\n".join([f"!{command}" for command in self.commands.keys()])
        )

    async def chat(self, bot):
        if conf.debug:
            print(f"chatBot incoming message: [{bot.rid}]{bot.msg}")

        # Map commands
        if bot.msg.startswith("!"):
            command = bot.msg[1:].split(" ")[0]
            if command in self.commands:
                await self.commands[command](bot, bot.rid)
        else:
            asyncio.create_task(self._query(bot, bot.rid, bot.msg))

    async def _query(self, bot, rid, msg) -> None:
        await bot.reply(await self.openai.submit(rid, msg))
