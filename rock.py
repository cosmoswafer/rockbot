#!/usr/bin/env python

import asyncio, datetime
from config import bot as conf
from bot.RocketChatBot import RocketChatBot
from bot.bot import bot
from bot.chat import chatBot


class botParser:
    def __init__(self, chat_bot: chatBot):
        self._chat = chat_bot

    async def chat(self, msg_bot: bot) -> None:
        await self._chat.update(msg_bot)


class rock:
    def __init__(self):
        self._registerBot()
        self._fireRocket()

    def _registerBot(self):
        self._bot = botParser(chatBot())

    def _fireRocket(self):
        print(f"Connecting to {conf.server} as {conf.username}...")
        self.rocket = RocketChatBot(
            conf.username, conf.password, server=conf.server, debug=conf.debug
        )
        asyncio.run(self.rocket.assignAtBot(self.bot_cb))
        # asyncio.run(self.rocket.assignRoom("atomkb", self.cb_atom))

    def start(self):
        """
        Main function which goes into async context.
        """
        RocketChatBot.launch(self.rocket)

    async def bot_cb(self, usr, rom, rid, txt):
        await self._bot.chat(bot(self.rocket, txt, rid))


if __name__ == "__main__":
    rock().start()
