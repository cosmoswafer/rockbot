import asyncio, aiosqlite, datetime
from dataclasses import dataclass, field, asdict
from typing import Literal
from util.fileOperation import fileOperation
from config import db as conf
from db.baseDb import baseDb

@dataclass
class ExpenseItem:
    purchase_date: datetime.date.fromisoformat
    item_name: str
    lifetime: int
    price: float
    category_a: str
    category_b: str
    serial_number: str = ""

    def __post_init__(self):
        if self.category_a not in conf.category:
            #raise ValueError("category_a should be one of the values in the pre-configured categories.")
            self.category_a = conf.category[0]
            #Another choice is set it as other category, better than exception
            #ASSUMED the first category is other

class expenseDb(baseDb):

    def __init__(self):
        self.db_file = fileOperation.fromDir(conf.data_dir, conf.db_file)
        self.db_name = self.db_file.db_file

    async def addExpense(self, expense_item: ExpenseItem):
        sql = """REPLACE INTO expense 
            (purchase_date, item_name, lifetime, price, category_a, category_b,
            void, t)
            VALUES
            (:purchase_date, :item_name, :lifetime, :price, :category_a, :category_b,
            FALSE, CURRENT_TIMESTAMP);"""
        await self.iOne(sql, asdict(expense_item))

    async def listExpense(self):
        sql = """SELECT ROWID, purchase_date, item_name, price, 
            IIF((n_yr-p_yr)*12+(n_mt-p_mt) < lifetime,
            ROUND(price * (lifetime-(n_yr-p_yr)*12+(n_mt-p_mt)) / lifetime,2),
            0) AS remaining_values,
            ROUND(price / lifetime, 2) AS monthly_cost,
            category_a, category_b, serial_number
            FROM (
                SELECT ROWID, purchase_date, item_name, price, lifetime,
                category_a, category_b, serial_number,
                strftime("%Y", purchase_date) AS p_yr,
                strftime("%m", purchase_date) AS p_mt,
                strftime("%Y", 'now') AS n_yr,
                strftime("%m", 'now') AS n_mt
                FROM expense WHERE void = FALSE
            ) T
            WHERE remaining_values > 0
            ORDER BY purchase_date DESC LIMIT 10;
        """
        async for r in self.qAll(sql):
            yield r
