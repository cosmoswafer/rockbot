#!/usr/bin/env python

import concurrent.futures
import asyncio
import websockets
import json
from util.decorators import defJson


class RocketChatBot:
    def __init__(self, user, password, server, debug=False):
        self._username = user
        self._password = password
        self._server = server
        self._debug = debug
        self._botname = "@" + str(user)

        # Enforce encryption
        self.uri = "wss://" + server + "/websocket"
        self.connect_msg = {"msg": "connect", "version": "1", "support": ["1"]}

        self.outgoing = []
        self._ws = None

        self.result = {}

        self.uid = ""

        self.cbdist = {}
        self.cbdist["ping"] = self._cb_ping
        self.cbdist["connected"] = self._cb_connected
        self.cbdist["result"] = self._rt_dispatch
        self.cbdist["changed"] = self._cb_changed

        self._atbot = None
        self._pmbot = None
        self._rooms = {}

    async def _wsLoop(self, ws):
        while True:
            await self._wsIncoming(ws)
            await asyncio.sleep(0.1)

    async def _wsIncoming(self, ws):
        self.ds = await ws.recv()
        if self._debug:
            print("WS<<<", self.ds)
        await self._dispatch_ds(self.ds)

    async def connect(self):
        async with websockets.connect(self.uri, ssl=True) as ws:
            self._ws = ws
            await self._send2ws(json.dumps(self.connect_msg))
            await self._wsLoop(ws)

    async def _dispatch_ds(self, ds):
        jds = json.loads(ds)
        self.msg = ""
        if "msg" in jds:
            self.msg = jds["msg"]
        if "result" in jds:
            self.result = jds["result"]

        cb = None
        if self.msg and self.msg in self.cbdist:
            cb = self.cbdist[self.msg]

        if cb:
            # print("Call ", cb.__name__)
            await cb()

    async def _cb_ping(self):
        await self._send2ws(json.dumps({"msg": "pong"}))

    async def _cb_connected(self):
        """
        Trying to authentice the server after the websocket connected.
        """
        payload = {
            "msg": "method",
            "method": "login",
            "id": "42",
            "params": [
                {"user": {"username": self._username}, "password": self._password}
            ],
        }
        await self._send2ws(json.dumps(payload))

    @defJson("")
    def _parse_msg_txt(self, json_data: dict) -> str:
        return json_data["fields"]["args"][0]["msg"]

    @defJson("")
    def _parse_room_id(self, json_data: dict) -> str:
        return json_data["fields"]["args"][0]["rid"]

    @defJson("")
    def _parse_sender_id(self, json_data: dict) -> str:
        return json_data["fields"]["args"][0]["u"]["_id"]

    @defJson("")
    def _parse_room_name(self, json_data: dict) -> str:
        return json_data["fields"]["args"][1]["roomName"]

    @defJson(False)
    def _in_channels(self, json_data: dict) -> bool:
        return (
            len(json_data["fields"]["args"]) > 1
            and "roonName" in json_data["fields"]["args"][1]
        )

    @defJson("")
    def _parse_sender_name(self, json_data: dict) -> str:
        return json_data["fields"]["args"][0]["u"]["username"]

    async def _cb_changed(self):
        jds = json.loads(self.ds)

        msg_txt = self._parse_msg_txt(jds)
        room_id = self._parse_room_id(jds)
        sender_id = self._parse_sender_id(jds)
        room_name = self._parse_room_name(jds) if self._in_channels(jds) else ""
        sender_name = self._parse_sender_name(jds)

        if sender_id == self.uid:
            return  # skip self message

        if room_name and msg_txt.startswith(self._botname):
            # AT the bot in channels
            msg_no_at = msg_txt.replace(self._botname, "")
            await self._atbot(sender_name, room_name, room_id, msg_no_at)
        elif self._rooms and room_name and room_name in self._rooms:
            # Private responses for some specific rooms
            await self._rooms[room_name](sender_name, room_name, room_id, msg_txt)
        elif not room_name:
            # Direct message which is missing the room name
            await self._atbot(sender_name, "DIRECT_MESSAGES", room_id, msg_txt)
        else:
            # Keep slince for other cases
            pass

    async def _rt_dispatch(self):
        if self.result:
            rt = self.result
            if "id" in rt and "token" in rt:
                print("Login successful!")
                print("ID: ", rt["id"])
                self.uid = rt["id"]
                print("Token: ", rt["token"])
                await self._gologin()
            self.result = {}

    async def _gologin(self):
        """
        Subscribing to the server to receive incoming messages.
        """
        payload = {
            "msg": "sub",
            "id": "ABCROCK",
            "name": "stream-room-messages",
            "params": ["__my_messages__", False],
        }
        await self._send2ws(json.dumps(payload))

    async def _send2ws(self, data):
        if not self._ws:
            return  # Supposed the porgram will exist if it loses the websocket connection

        if self._debug:
            print("WS>>>", data)
        await self._ws.send(data)

    async def sendMsg(self, rid, msg):
        payload = {
            "msg": "method",
            "method": "sendMessage",
            "id": "42",
            "params": [{"rid": rid, "msg": msg}],
        }
        await self._send2ws(json.dumps(payload))

    async def assignAtBot(self, cb):
        self._atbot = cb

    async def assignRoom(self, room, cb):
        """
        Assign to only response in the specific room list.
        If it's empty, which is the default, this bot will response to everyone.
        (But so far it disabled responsing to everyone.)

        room should be a string, which is the roomName


        Call back function cb:
        cb(from_user, room_name, room_id, message)
        """
        # self._rooms.append({"name": room, "cb": cb})
        if room in self._rooms:
            print(
                f"{room} call back already exists and will be overridden by the new cb!"
            )
        self._rooms[room] = cb

    @staticmethod
    def launch(rocket):
        try:
            asyncio.run(rocket.connect())
        except KeyboardInterrupt:
            print("Quit the bot now...")


if __name__ == "__main__":
    rocket = RocketChatBot("botname", "password", server="hostname.com")
    # asyncio.run(rocket.start(None))
    RocketChatBot.launch(rocket)
