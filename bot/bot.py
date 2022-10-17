class bot:
    """
    Helper class for subscribers to send back messages.
    """

    def __init__(self, rocket, args, rid):
        self.rocket = rocket
        self.args = args
        self.rid = rid
        self.txt = ""

    async def reply(self, msg):
        await self.rocket.sendMsg(self.rid, msg)

    async def replyQ(self, msg):
        msg_q = f"```\n{msg}\n```"
        await self.rocket.sendMsg(self.rid, msg_q)



