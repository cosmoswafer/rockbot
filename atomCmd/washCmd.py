#!/usr/bin/env python

import asyncio, datetime
from cmdAbc import cmd
from db.fuelDb import fuelDb

class washCmd(cmd):
    
    def __init__(self, parser):
        self.db = fuelDb()

        self._initParser(parser)

    def _initParser(self, parser):
        self.parser = parser

        self.parser.add_argument("desc", type=str, help="內容")
        self.parser.add_argument("c", type=float, help="價錢")
        self.parser.add_argument("mop", type=str, choices=["mop"], help="Placeholder")
        self.parser.add_argument("--date" \
                , type=datetime.date.fromisoformat \
                , default=datetime.date.today().isoformat() \
                , help="日期，ISO format, i.e. YYYY-MM-DD" )
        self.parser.add_argument("--report", action="store_true", help="顯示報告")

        self.parser.set_defaults(func=self.update)

    async def update(self, bot):
        if not bot.args.report:
            await self._updateWash(bot)
        await bot.reply("**清潔美容**")
        await bot.replyQ(await self._reportWash())

    async def _updateWash(self, bot):
        e = \
            (bot.args.c \
            ,bot.args.date \
            ,bot.args.desc)
        await self.db.insertWash(e)

    async def _reportWash(self):
        r = []
        async for i in self.db.reportWash():
            r.append(f'MOP {i["c"]:4.0f} @{i["d"]}: {i["desc"]}')
        return "\n".join(r)
