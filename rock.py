#!/usr/bin/env python

import asyncio, datetime
from config import bot as conf
from util.theArgParse import theArgParse
from bot.RocketChatBot import RocketChatBot
from comnCmd.countdCmd import countd
from comnCmd.gifCmd import gifCmd
from atomCmd.fuelCmd import fuelCmd
from atomCmd.mantCmd import mantCmd
from atomCmd.washCmd import washCmd
from atomCmd.vdiCmd import vdiCmd
from comnCmd.duckCmd import duckCmd
from atomCmd.assetCmd import assetCmd
from comnCmd.chatCmd import chatCmd

from bot.bot import bot


class cmdParser:
    def __init__(self, cmds):
        """
        name is the parser's name in string.
        cmds is a dictionary with the commands.

        Example of a cmds: {"count": countd}, here countd is a cmd object.
        """
        self.parser = theArgParse.Parser4Text()
        self.subparser = self.parser.add_subparsers(
            dest="cmd", help="All available cmmands"
        )
        self.subparser.required = True
        self.cmds = []

        self.last_errmsg = ""

        for i in cmds:
            cmd = cmds[i]
            sub_parser = self.subparser.add_parser(
                i, add_help=False, help=f"{i} command."
            )
            self.cmds.append(cmd(sub_parser))


class rock:
    def __init__(self):
        self.actions = ["5min", "30min", "50min", "cd"]
        self.units = {"min": 60, "sec": 1}

        self._crtCommands()
        self._fireRocket()

    def _crtCommands(self):
        bot_cmds = {"count": countd, "gif": gifCmd, "duck": duckCmd, "chat": chatCmd}
        atom_cmds = {
            "fuel": fuelCmd,
            "mant": mantCmd,
            "wash": washCmd,
            "vdi": vdiCmd,
            "asset": assetCmd,
        }
        self._bot = cmdParser(bot_cmds)
        self._atom = cmdParser(atom_cmds)

    def _fireRocket(self):
        self.rocket = RocketChatBot(
            conf.username, conf.password, server=conf.server, debug=conf.debug
        )
        asyncio.run(self.rocket.assignAtBot(self.cb_bot))
        asyncio.run(self.rocket.assignRoom("atomkb", self.cb_atom))

    def start(self):
        """Main function which goes into async context."""
        # asyncio.run(self.rocket.start())
        RocketChatBot.launch(self.rocket)

    async def _parseArg(self, cmd, txt):
        err_msg = ""
        args = None

        try:
            # args = cmd.parser.parse_args(shlex.split(txt))
            args = cmd.parser.parse_args(txt.strip().split(" "))
        except (theArgParse.ArgumentError, UserWarning) as e:
            args = None
            err_msg = str(e) + "\n" + cmd.parser.format_usage()
            err_msg += "".join([p.parser.format_usage() for p in cmd.cmds])
            # await self.rocket.sendMsg(rid, err_msg)
        except SystemExit as e:
            # Test args doesn't respect to the exit_on_error
            args = None
        finally:
            cmd.last_errmsg = err_msg

        return args

    async def cb_bot(self, usr, rom, rid, txt):
        args = await self._parseArg(self._bot, txt)

        if args:
            await args.func(bot(self.rocket, args, rid))
        else:
            await self.rocket.sendMsg(rid, self._bot.last_errmsg)

    async def cb_atom(self, usr, rom, rid, txt):
        args = await self._parseArg(self._atom, txt)

        if args:
            await args.func(bot(self.rocket, args, rid))
        else:
            await self.rocket.sendMsg(rid, self._atom.last_errmsg)


if __name__ == "__main__":
    rock().start()
