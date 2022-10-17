#!/usr/bin/env python

import asyncio, datetime
from cmdAbc import cmd

class countd(cmd):
    units = {"min": 60, "sec": 1}

    def __init__(self, parser):
        self.parser = parser
        self.parser.add_argument("time", type=int)
        self.parser.add_argument("unit", type=str, choices=countd.units.keys())
        self.parser.add_argument("--title", type=str)
        self.parser.set_defaults(func=self.update)

    async def update(self, bot):
        unit = self.units[bot.args.unit]
        await self.countdCmd(bot, bot.args.time*unit)


    async def countdCmd(self, bot, s):
        title_txt = f"{s/60} minutes - "
        if bot.args.title:
            title_txt = f"{bot.args.title} - "
        asyncio.create_task(self.cdAft(bot, s, f"{title_txt}Time's up!"))
        await bot.reply(f"{title_txt}Count down {s/60} minutes started.")

    async def cdAft(self, bot, s, t):
        await self.countDown(s)
        await bot.reply(t)

    async def countDown(self, s):
        start = datetime.datetime.now()

        dt = datetime.datetime.now() - start
        while dt.seconds <= s:
            dt = datetime.datetime.now() - start
            await asyncio.sleep(1)

