#!/usr/bin/env python

import asyncio, datetime
from cmdAbc import cmd
from db.fuelDb import fuelDb

class mantCmd(cmd):
    
    def __init__(self, parser):
        self.db = fuelDb()

        self._initParser(parser)

    def _initParser(self, parser):
        """
        Add the argument into this sub parser.
        Nested sub parser will not work because of the help message.
        """
        self.parser = parser

        self.parser.add_argument("desc", type=str, help="內容")
        self.parser.add_argument("c", type=float, help="價錢")
        self.parser.add_argument("mop", type=str, choices=["mop"], help="Placeholder")
        self.parser.add_argument("--km", default=-1, type=int, help="行駛公里總數")
        self.parser.add_argument("--date" \
                , type=datetime.date.fromisoformat \
                , default=datetime.date.today().isoformat() \
                , help="日期，ISO format, i.e. YYYY-MM-DD" )
        self.parser.add_argument("--report", action="store_true", help="顯示報告")

        self.parser.set_defaults(func=self.update)

    async def update(self, bot):
        if not bot.args.report:
            await self._updateMant(bot)
        await bot.reply("**維修保養**")
        await bot.replyQ(await self._reportMant())

    async def _updateMant(self, bot):
        e = \
            (bot.args.desc \
            ,bot.args.c \
            ,bot.args.date \
            )
        km = bot.args.km

        await self.db.insertMant(e, km)

    async def _reportMant(self):
        r = []
        async for i in self.db.reportMant():
            r.append(f'日期：{i["d"]} {i["km"]:7d}km MOP{i["c"]:9.2f} 項目：{i["desc"]}')
        return "\n".join(r)
