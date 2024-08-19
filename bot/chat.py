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
            "model": {"fnc": self.model, "desc": "Switch AI model"},
            "history": {"fnc": self.history, "desc": "Display chat history statictics"},
            "clear": {"fnc": self.clear, "desc": "Clear chat history"},
        }

    async def clear(self, bot):
        self.openai.histories[bot.rid] = []
        if len(bot.msg.split(" ")) >= 2:
            model_id = bot.msg.split(" ")[1]
            await self._switch_model(model_id)
        await bot.reply(f"History cleared, current model: {conf.model}")

    async def help(self, bot):
        commands = "\n".join(
            [
                f"!{command}: {self.commands[command]['desc']}"
                for command in self.commands.keys()
            ]
            + ["All available models:"]
            + [
                f"- {model_id} => {model_code}"
                for model_id, model_code in conf.models.__dict__.items()
            ]
        )
        await bot.reply(f"Commands:\n{commands}")

    async def _switch_model(self, model_id):
        model_dict = conf.models.__dict__
        default_model, _ = next(iter(model_dict.items()))
        model_code = (
            model_dict[model_id] if model_id in model_dict else default_model
        )
        if model_code != conf.model:
            conf.model = model_code

    async def model(self, bot):
        model_code = conf.model
        model_id = model_code
        if len(bot.msg.split(" ")) >= 2:
            model_id = bot.msg.split(" ")[1]
            await self._switch_model(model_id)
        await bot.reply(f"Using model {model_id} => {conf.model}")

    async def history(self, bot):
        history_size = len(self.openai.histories[bot.rid]) if bot.rid in self.openai.histories else 0
        history_len = len(str(self.openai.histories[bot.rid])) if bot.rid in self.openai.histories else 0
        await bot.reply(
            f"Current history size/limit: **{history_size}**/{conf.max_history_size} "
            f"(**{history_len}**/{conf.max_text_length} characters)" 
        )

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
