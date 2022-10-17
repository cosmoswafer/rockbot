#!/usr/bin/env python

import asyncio
from cmdAbc import cmd
from db.gifDb import gifDb

class gifCmd(cmd):

    def __init__(self, parser):
        self.db = gifDb()

        self.parser = parser
        self.parser.add_argument("name", type=str, nargs="?")
        #self.parser.add_argument("url", type=str, nargs="?")
        group = self.parser.add_mutually_exclusive_group()
        group.add_argument("--add", dest="url", type=str)
        group.add_argument("--delete", action="store_true")
        self.parser.set_defaults(func=self.update)

    async def update(self, bot):
        if bot.args.name and bot.args.url:
            await bot.reply(await self._addGif(bot.args.name, bot.args.url))
        elif not bot.args.name and not bot.args.url:
            await bot.reply(await self._listGif())
        elif bot.args.delete and bot.args.name:
            await bot.reply(await self._deleteGif(bot.args.name))
        elif bot.args.name:
            await bot.reply(await self._showGif(bot.args.name))
        else:
            await bot.reply("Wrong usage")

    async def _addGif(self, name, url):
        """
        Rocket Chat Message formatting:
        ![Alt text](https://rocket.chat/favicon.ico)
        """
        await self.db.insertGif(name, url)
        return f"Added gif: {name}@{url}"

    async def _listGif(self):
        #gifs = list(await self.db.listGif())
        gifs = [f'* {x["name"]}' async for x in self.db.listGif()]
        #msg = "\n".join([f"* {name}" for name in reversed(gifs)])
        return "\n".join(reversed(gifs))

    async def _showGif(self, name):
        e = await self.db.selectGif(name)
        if e:
            return f"![{e['name']}]({e['url']})"
        else:
            return f"Gif (name={name}) was not found!"

    async def _deleteGif(self, name):
        e = await self.db.selectGif(name)
        if e:
            url = e["url"]
            await self.db.deleteGif(name)
            return f"Removing gif {name} ({url}) from database..."
        else:
            return f"The gif {name} was not found"
