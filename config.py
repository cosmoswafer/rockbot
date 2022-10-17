#!/usr/bin/env python

import os, json
from types import SimpleNamespace

def getVar(var, default_var = None):
    if var in os.environ:
        return os.environ[var]
    else:
        return default_var

config_json_file = getVar("CONFIG_JSON", "config.json")

print(f"Environment variable CONFIG_JSON={config_json_file}")
print(f"Trying to load configuration from json file: {config_json_file}")
with open(config_json_file, "r") as f:
    _config = json.load(f, object_hook=lambda d: SimpleNamespace(**d))
    db = _config.db
    bot = _config.bot

