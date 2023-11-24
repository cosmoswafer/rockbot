from abc import ABC, abstractmethod


class cmd(ABC):
    @abstractmethod
    async def update(self, bot, rid):
        pass
