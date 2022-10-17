#!/bin/bash

#sudo -u nginx git config user.email "nginx@atom.local"
#sudo -u nginx git config user.name "Nginx"

sqlite3 MQ-7860.db .dump > MQ-7860.sql
git commit -am "Table dump"
