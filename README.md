# Rocket Chat bot with sqlite database

## Python packages

 * websockets
 * aiosqlite

## Database schema

Fuel database:

```sql
CREATE TABLE fuel (km INT(11), l FLOAT(5.2), c FLOAT(7.2), d DATE, void INT(1) default FALSE);
CREATE TABLE mant (desc TEXT, c FLOAT(7.2), km INT(11), d DATE, void INT(1) default FALSE);
CREATE TABLE wash (c FLOAT(7.2), d DATE, desc TEXT, void INT(1) default FALSE);
```

Expense tables:

```sql
CREATE TABLE expense 
    (purchase_date DATE
    ,item_name TEXT
    ,lifetime INT(5) --Unit: months
    ,price FLOAT(13.2) --MOP only
    ,category_a TEXT
    ,category_b TEXT
    ,serial_number TEXT DEFAULT ''
    ,void INT(1) DEFAULT FALSE
    ,t TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    );
```

Gif database:

```sql
CREATE TABLE gif 
    (name TEXT PRIMARY KEY
    ,url TEXT
    ,t TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ,void INT(1) DEFAULT FALSE
    );
```

## Implementation 

Nither library or framework, just a programming method to quickly implement a *minimalism* information system.

### General technique

* Python `argparse` application in bot commands
* Python async programming
* Dataclass and database conecting
* Simple Bot logic
* Test driven development

### Tricky points
