import argparse

class theArgParse(argparse.ArgumentParser):
    ArgumentError = argparse.ArgumentError

    def error(self, message):
        #raise argparse.ArgumentError(message)
        raise UserWarning(message)

    @classmethod
    def Parser4Text(cls):
        return cls(prog="", add_help=False, exit_on_error=False)

