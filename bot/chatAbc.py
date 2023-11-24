from abc import ABC, abstractmethod


class chatABC(ABC):
    @abstractmethod
    async def chat(self, bot, rid):
        pass
