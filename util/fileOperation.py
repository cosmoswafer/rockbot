#!/usr/bin/env python

from datetime import date
from pathlib import Path
import shutil, zlib

class fileOperation:

    def __init__(self, file_name):
        self.file_name = file_name
        self.db_file = Path(self.file_name)
        d = date.today()
        new_ext = f".{d.year:04}{d.month:02}{d.day:02}"
        """
        self.bk_file = Path(".".join(["",self.file_name,new_ext]))
        self.bk_filz = Path(".".join(["",self.file_name,new_ext,"gz"]))
        """
        self.bk_file = self.db_file.with_suffix(self.db_file.suffix+new_ext)
        self.bk_filz = self.db_file.with_suffix(self.db_file.suffix+new_ext+".gz")

    def backup(self):
        shutil.copy(self.db_file, self.bk_file)

    def backupZ(self):
        with open(self.db_file, "rb") as f:
            with open(self.bk_filz, "wb") as t:
                t.write(zlib.compress(f.read()))
        shutil.copymode(self.db_file, self.bk_filz)

    @classmethod
    def fromDir(cls, dir_name, file_name):
        db_file = Path(dir_name) / Path(file_name)
        return cls(db_file)
