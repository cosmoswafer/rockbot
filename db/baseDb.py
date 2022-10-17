import asyncio, aiosqlite, datetime

class baseDb:

    def __init__(self):
        """
        Please override this method to create the corrent database name for subclasses.
        """
        self.db_name = "DATABASE_FILENAME"

    async def qOne(self, sql, entry=None):
        r = None
        async with aiosqlite.connect(self.db_name) as db:
            db.row_factory = aiosqlite.Row
            async with db.execute(sql, entry) as cursor:
                r = await cursor.fetchone()

        return r

    async def iOne(self, sql, entry):
        """
        async with aiosqlite.connect(self.db_name) as db:
            await db.execute(sql, entry)
            await db.commit()
        """
        last_row = 0
        async with aiosqlite.connect(self.db_name) as db:
            async with db.execute(sql, entry) as cursor:
                last_row = cursor.lastrowid
            await db.commit()

        return last_row

    async def qAll(self, sql, entry=None):
        async with aiosqlite.connect(self.db_name) as db:
            db.row_factory = aiosqlite.Row
            async with db.execute(sql, entry) as cursor:
                async for row in cursor:
                    yield row
