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

        self.subparser = self.parser.add_subparsers(dest="asset_cmd", help="Asset sub commands")
        self.subparser.required = True
        self.add_parser = self.subparser.add_parser("add",add_help=False, help="Add new asset")
        self.add_parser.add_argument("--date" \
                , type=datetime.date.fromisoformat \
                , default=datetime.date.today().isoformat() \
                , help="日期，ISO format, i.e. YYYY-MM-DD" )
        self.add_parser.add_argument("--item", dest="item", type=str, help="內容")
        self.add_parser.add_argument("--price", dest="price", type=float, help="價錢")
        self.add_parser.add_argument("--period", dest="period", type=float, help="Deprecation period, in years")
        self.add_parser.add_argument("--acct", dest="acct", choices=["A","B","C"], help="Main account")
        self.add_parser.add_argument("--cat", dest="cat", type=str, help="Catagory")

        self.delete_parser = self.subparser.add_parser("delete", add_help=False, help="Delete existing asset")
        self.delete_parser.add_argument("asset_id", type=int, help="Asset id to be deleted")

        #self.parser.add_argument("--report", action="store_true", help="顯示報告")

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
        """
        e = {
            "name": args.item,
            "price": args.price,
            "period": args.period,
            "account": args.acct,
            "catagory": args.cat
        }
        """
        return str(args)

