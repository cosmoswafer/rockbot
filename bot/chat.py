#!/usr/bin/env python

import asyncio
from config import openai as conf
from bot.openai import OpenAi


class chatBot:
    def __init__(self, openai=OpenAi()):
        self.openai = openai
        self.commands = {
            "help": {"fnc": self.help, "desc": "Display available commands"},
            "clear": {"fnc": self.clear, "desc": "Clear chat history"},
        }

    async def clear(self, bot):
        self.openai.histories[bot.room_id] = []
        await bot.reply("History cleared")

    async def help(self, bot):
        commands = "\n".join(
            [
                f"!{command}: {self.commands[command]['desc']}"
                for command in self.commands.keys()
            ]
        )
        await bot.reply(f"Commands:\n{commands}")

    async def chat(self, bot):
        if conf.debug:
            print(f"chatBot incoming message: [{bot.rid}]{bot.msg}")

        # Map commands
        if bot.msg.startswith("!"):
            command = bot.msg[1:].split(" ")[0]
            if (
                command in self.commands.keys()
            ):  # Fix: Access the keys of the commands dictionary
                await self.commands[command]["fnc"](
                    bot
                )  # Fix: Call the function stored in the "fnc" key
        else:
            asyncio.create_task(self._query(bot, bot.rid, bot.msg))

    async def _query(self, bot, rid, msg) -> None:
        await bot.reply(await self.openai.submit(rid, msg))
