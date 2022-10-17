#!/usr/bin/env python

import asyncio
from db.expenseDb import expenseDb, ExpenseItem
from typing import Literal
from dataclasses import dataclass, field, asdict

class ts:
    def __init__(self):
        self.db = expenseDb()

    async def main(self):
        a = ExpenseItem("2020-02-27", "First item", 18, 777.90, "個人", "測試")
        print(a)
        b = ExpenseItem("2021-02-27", "First item", 18, 777.90, "非個人", "測試")
        print(b)
        print(asdict(a))

        #await self.db.addExpense(a)
        #await self.db.addExpense(b)

        async for i in self.db.listExpense():
            print(">>", [x for x in i])

t = ts()
asyncio.run(t.main())

