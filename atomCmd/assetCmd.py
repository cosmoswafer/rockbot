#!/usr/bin/env python

import asyncio, datetime
from cmdAbc import cmd
from db.assetDb import assetDb

class assetCmd(cmd):

    def __init__(self, parser):
        self.db = assetDb()

        self._initParser(parser)

    def _initParser(self, parser):
        self.parser = parser

        self.parser.add_argument("--date" \
                , type=datetime.date.fromisoformat \
                , default=datetime.date.today().isoformat() \
                , help="日期，ISO format, i.e. YYYY-MM-DD" )
        self.parser.add_argument("--item", dest="item", type=str, help="內容")
        self.parser.add_argument("--price", dest="price", type=float, help="價錢")
        self.parser.add_argument("--period", dest="period", type=float, hepl="Deprecation period, in years"))
        self.parser.add_argument("--acct", dest="acct", choices=["A","B","C"], hlep="Main account")
        self.parser.add_argument("--cat", dest="cat", type=str, help="Catagory")
        #self.parser.add_argument("--report", action="store_true", help="顯示報告")
        group = self.parser.add_mutually_exclusive_group()
        group.add_argument("--add", action="store_true", default=True)
        group.add_argument("--delete", action="store_true")

        self.parser.set_defaults(func=self.update)

    async def update(self, bot):
        """
        if not bot.args.report:
            await self._updateWash(bot)
        await bot.reply("**清潔美容**")
        await bot.replyQ(await self._reportWash())
        """
        await bot.reply(str(await self._asset(bot.args)))

    async def _asset(self, args):
        e = {
            name: args.item,
            price: args.price,
            period: args.period,
            account: args.acct,
            catagory: args.cat
        }
        return e

