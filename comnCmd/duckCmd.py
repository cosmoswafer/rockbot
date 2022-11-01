#!/usr/bin/env python

import asyncio
from cmdAbc import cmd

class gifCmd(cmd):

    def __init__(self, parser):
        self.parser = parser
        self.parser.add_argument("name", type=str, nargs="?")
        #self.parser.add_argument("url", type=str, nargs="?")
        group = self.parser.add_mutually_exclusive_group()
        group.add_argument("-d", "--definition", dest="Check the definition")
        group.add_argument("keywords", dest="Query keywords", type=str, nargs="+")
        self.parser.set_defaults(func=self.update)

    async def update(self, bot):
        if bot.args.definition:
            await bot.reply("Definition")
        await bot.reply(await self._query(bot.args.keywords))

    async def _query(self, keywords):
        return " ".join(keywords)
