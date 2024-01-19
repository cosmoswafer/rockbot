#!/usr/bin/env python

from util.logger import logger
import os, json
from types import SimpleNamespace


def getVar(var, default_var=None):
    if var in os.environ:
        return os.environ[var]
    else:
        return default_var


_config_json_file = getVar("CONFIG_JSON", "config.json")

logger.debug(f"Environment variable CONFIG_JSON={_config_json_file}")
with open(_config_json_file, "r") as f:
    _config = json.load(f, object_hook=lambda d: SimpleNamespace(**d))
    db = _config.db
    bot = _config.bot
    openai = _config.bot.openai
logger.info(f"Loaded configuration from json file: {_config_json_file}")
