import asyncio, aiosqlite
from util.fileOperation import fileOperation
from config import db as conf
from db.baseDb import baseDb

class gifDb(baseDb):

    def __init__(self):
        self.db_file = fileOperation.fromDir(conf.data_dir, conf.dbat_file)
        self.db_name = self.db_file.db_file

    async def insertGif(self, name, url):
        """
        CREATE TABLE gif 
            (name TEXT PRIMARY KEY
            ,url TEXT
            ,t TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            ,void INT(1) DEFAULT FALSE
            );
        """
        db_entry = {"name": name, "url": url}
        sql = "REPLACE INTO gif (name, url, t, void) VALUES (:name, :url, CURRENT_TIMESTAMP, FALSE);"
        await self.iOne(sql, db_entry)

    async def selectGif(self, name):
        sql = "SELECT name, url FROM gif WHERE name = :name AND void = FALSE;"
        return await self.qOne(sql, (name,))

    async def listGif(self, limit=10):
        sql = "SELECT name FROM gif WHERE void = FALSE ORDER BY t DESC LIMIT :limit;"
        async for i in self.qAll(sql, (limit,)):
            yield i
        
    async def deleteGif(self, name):
        sql = "UPDATE gif SET void = TRUE, t = CURRENT_TIMESTAMP WHERE name = :name;"
        await self.iOne(sql, (name,))
