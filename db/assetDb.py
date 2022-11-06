import asyncio, aiosqlite, datetime
from util.fileOperation import fileOperation
from config import db as conf
from db.baseDb import baseDb

class assetDb(baseDb):

    def __init__(self):
        self.db_file = fileOperation.fromDir(conf.data_dir, conf.db_file)
        self.db_name = self.db_file.db_file

    async def insertFuel(self, entry):
        """
        CREATE TABLE fuel (km INT(11), l FLOAT(5.2), c FLOAT(7.2), d DATE);
        """
        #self.db_file.backupZ()

        sql = "INSERT INTO fuel (km, l, c, d) VALUES (:km, :l, :c, :d);"
        await self.iOne(sql, entry)

    async def reportFuel(self):
        sql = "SELECT * FROM ( \
                SELECT km, l, c, d FROM fuel \
                WHERE void <> 1 \
                ORDER BY d DESC LIMIT 11) \
                AS t \
                ORDER BY d ASC;"

        async for r in self.qAll(sql):
            yield r

    async def insertWash(self, entry):
        """
        CREATE TABLE wash (c FLOAT(7.2), d DATE, desc TEXT);
        """
        sql = "INSERT INTO wash (c, d, desc) VALUES (:c, :d, :desc);"
        await self.iOne(sql, entry)

    async def reportWash(self):
        sql = "SELECT * FROM ( \
                SELECT c, d, desc FROM wash WHERE void <> 1 \
                ORDER BY d DESC LIMIT 10) \
                AS t ORDER BY d ASC;"
        async for r in self.qAll(sql):
            yield r

    async def insertMant(self, entry, km=-1):
        """
        CREATE TABLE mant (desc TEXT, c FLOAT(7.2), km INT(11), d DATE);
        """
        #self.db_file.backupZ()

        last_row = await self._insertMantEntry(entry)
        if km == -1 or km == None:
            new_km = await self._getMaxKm()
        else:
            new_km = km
        await self._updateMantKm(last_row, new_km)

    async def _insertMantEntry(self, entry):
        sql = "INSERT INTO mant (desc, c, d) VALUES (:desc, :c, :d);"
        last_row = await self.iOne(sql, entry)
        return last_row

    async def _getMaxKm(self):
        sql = "SELECT MAX(km) FROM fuel WHERE void <> 1;"
        fuel_km = await self.qOne(sql)

        if fuel_km:
            return fuel_km[0]
        else:
            return fuel_km

    async def _updateMantKm(self, last_row, km):
        sql = "UPDATE mant SET km = :km WHERE rowid = :rowid;"
        await self.iOne(sql, (km, last_row))

    async def reportMant(self):
        sql = "SELECT desc, c, km, d FROM mant WHERE void <> 1 ORDER BY d ASC;"
        async for r in self.qAll(sql):
            yield r

    async def reportStatis(self):
        r = {}
        r["age"] = await self._getAge()
        r["miles"] = await self._getMiles()
        r["consumption"] = await self._getConsumption() / r["age"]
        r["avg_spends"] = await self._getSpends() / r["age"]
        return r

    async def _getAge(self):
        sql = "SELECT MIN(MIN(f.d), MIN(m.d)) FROM fuel f, mant m WHERE f.void <> 1 AND m.void <> 1;"
        r = await self.qOne(sql)
        if r:
            ground_date = r[0]
        else:
            ground_date = r


        try:
            d1 = datetime.date.fromisoformat(ground_date)
        except (ValueError, TypeError):
            d1 = datetime.date.today()
        d2 = datetime.date.today()
        used_months = (d2.year - d1.year) * 12 - (d2.month - d1.month)
        if used_months <= 0:
            used_months = 1 #To avoid division by zero for calculation later
            #Minimum age is 1 month

        return used_months

    async def _getMiles(self):
        sql = "SELECT MAX(MAX(f.km), MAX(m.km)) FROM fuel f, mant m WHERE f.void <> 1 AND m.void <> 1;"
        miles = await self.qOne(sql)
        if miles:
            return miles[0]
        else:
            return miles 

    async def _getConsumption(self):
        sql = "SELECT SUM(c) FROM fuel WHERE void <> 1;"
        consumption = await self.qOne(sql)
        if consumption:
            return consumption[0]
        else:
            return consumption

    async def _getSpends(self):
        sql = """SELECT SUM(c) FROM
    (   SELECT c FROM fuel WHERE void <> 1
        UNION ALL
        SELECT c FROM mant WHERE void <> 1
        UNION ALL
        SELECT c FROM wash WHERE void <> 1
    ) t;"""

        spends = await self.qOne(sql)
        if spends:
            return spends[0]
        else:
            return spends 
