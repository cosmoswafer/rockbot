class bot:
    """
    Helper class for subscribers to send back messages.
    """

    def __init__(self, rocket, msg, rid):
        self.rocket = rocket
        self.msg = msg.strip()  # Trim the msg automatically
        self.rid = rid
        self.txt = ""

    async def typing(self, state: bool):
        await self.rocket.notifyTyping(self.rid, state)

    async def reply(self, msg):
        await self.rocket.sendMsg(self.rid, msg)

    async def replyQ(self, msg):
        msg_q = f"```\n{msg}\n```"
        await self.rocket.sendMsg(self.rid, msg_q)
