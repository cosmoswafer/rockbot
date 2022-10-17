from abc import ABC, ABCMeta, abstractmethod

class cmd(ABC):
    @abstractmethod
    def __init__(self, parser):
        """
        #At least should containt the following:
        self.parser = parser
        self.parser.set_defaults(func=self.update)
        """
        pass

    @abstractmethod
    async def update(self, bot, rid):
        pass
 
