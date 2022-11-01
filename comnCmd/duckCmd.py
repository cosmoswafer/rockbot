#!/usr/bin/env python

import asyncio
from cmdAbc import cmd

class duckCmd(cmd):

    def __init__(self, parser):
        self.parser = parser
        self.parser.add_argument("-d", "--definition", help="Check the definition", action="store_true")
        self.parser.add_argument("keywords", help="Query keywords", type=str, nargs="*")
        self.parser.set_defaults(func=self.update)

    async def update(self, bot):
        if bot.args.definition:
            await bot.reply("Definition")
        await bot.reply(await self._query(bot.args.keywords))

    async def _query(self, keywords):
        return " ".join(keywords)
