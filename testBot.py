from bot.RocketChatBot import RocketChatBot
from bot.bot import bot
from bot.chat import chatBot
from util.config import bot as conf


class Test:
    def __init__(sefl):
        pass

    def testAll(self):
        self.testBot()
        print("All tests passed")

    def testBot(self):
        rocket = RocketChatBot(conf.username, conf.password, server=conf.server)
        RocketChatBot.launch(rocket)
        print("Bot launched")
        print("Test passed")


if __name__ == "__main__":
    Test().testAll()
