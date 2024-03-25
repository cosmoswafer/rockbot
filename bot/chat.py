#!/usr/bin/env python

from util.logger import logger
import asyncio
from util.config import openai as conf
from bot.openai import OpenAi


class chatBot:
    def __init__(self, openai=OpenAi()):
        self.openai = openai
        self.commands = {
            "help": {"fnc": self.help, "desc": "Display available commands"},
            "model": {"fnc": self.model, "desc": "Change AI model"},
            "clear": {"fnc": self.clear, "desc": "Clear chat history"},
        }

    async def clear(self, bot):
        self.openai.histories[bot.rid] = []
        await bot.reply("History cleared")

    async def help(self, bot):
        commands = "\n".join(
            [
                f"!{command}: {self.commands[command]['desc']}"
                for command in self.commands.keys()
            ]
            + ["All available models:"]
            + [f"{model_id}" for model_id, _ in conf.models.__dict__.items()]
        )
        await bot.reply(f"Commands:\n{commands}")

    async def model(self, bot):
        model_dict = conf.models.__dict__
        default_model, _ = next(iter(model_dict.items()))
        if len(bot.msg.split(" ")) >= 2:
            model_id = bot.msg.split(" ")[1]
            model_name = (
                model_dict[model_id] if model_id in model_dict else default_model
            )
        else:
            model_id = ""
            model_name = conf.model
        if model_name != conf.model:
            conf.model = model_name
        await bot.reply(f"Using model {model_id}:{model_name} => {conf.model}")

    async def chat(self, bot):
        logger.debug(f"chatBot incoming message: [{bot.rid}]{bot.msg}")

        # Map commands
        if bot.msg.startswith("!"):
            command = bot.msg[1:].split(" ")[0]
            if (
                command in self.commands.keys()
            ):  # Fix: Access the keys of the commands dictionary
                await self.commands[command]["fnc"](
                    bot
                )  # Fix: Call the function stored in the "fnc" key
        else:
            asyncio.create_task(self._query(bot, bot.rid, bot.msg))

    async def _query(self, bot, rid, msg) -> None:
        await bot.reply(await self.openai.submit(rid, msg))
