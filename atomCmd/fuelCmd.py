#!/usr/bin/env python

import asyncio, datetime
from cmdAbc import cmd
from db.fuelDb import fuelDb

class fuelCmd(cmd):

    def __init__(self, parser):
        self.db = fuelDb()

        self._initParser(parser)

    def _initParser(self, parser):
        self.parser = parser

        self.parser.add_argument("km", type=int, help="行駛公里總數")
        self.parser.add_argument("kmps", choices=["km"], help="Placeholder")
        self.parser.add_argument("L", type=float, help="公升")
        self.parser.add_argument("Lps", choices=["L"], help="Placeholder")
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
            await self._updateFuel(bot)
        await bot.reply("**油耗記錄**")
        await bot.replyQ(await self._reportFuel())
        await bot.reply("車牌： MQ-7860")
        await bot.replyQ(await self._reportStatis())

    async def _updateFuel(self, bot):
        e = \
            (bot.args.km \
            ,bot.args.L  \
            ,bot.args.c  \
            ,bot.args.date  \
            )
        await self.db.insertFuel(e)

    async def _reportFuel(self):
        first_row = True
        r = []
        async for i in self.db.reportFuel():
            if first_row:
                first_row = False
            else:
                d1 = datetime.date.fromisoformat(last_d)
                d2 = datetime.date.fromisoformat(i["d"])
                dd = d2 - d1
                mils = i["km"] - last_km
                try:
                    lpkm = i["l"] / mils * 100
                except ZeroDivisionError:
                    lpkm = 0
                lsdt = dd.days
                try:
                    avgm = mils / lsdt
                except ZeroDivisionError:
                    avgm = 0

                r1 = f'[{i["d"]}, {i["km"]:7d}km, {i["l"]:5.2f}L, MOP {i["c"]:6.2f}]'
                r2 = f'油耗： {lpkm:5.2f}L/100km ~{mils:4d}km ({lsdt:3d}日@{avgm:5.2f}km)'
                r.append(" ".join([r1, r2]))
            last_d = i["d"]
            last_km = i["km"]

        return "\n".join(r)

    async def _reportStatis(self):
        rpt = await self.db.reportStatis()
        r = []
        r.append(f'車齡{rpt["age"]:d}個月')
        r.append(f'總行駛里數：{rpt["miles"]:d}km')
        r.append(f'平均每月費月： MOP {rpt["avg_spends"]:.0f}')
        r.append(f'每月油費： MOP {rpt["consumption"]:.2f}')
        return "\n".join(r)
