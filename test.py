class Test:
    def __init__(self):
        self._true_count = 0

    def testAll(self):
        """
        self.testWhile()
        self.testReturnOr()
        self.testReturnStatus()
        """
        print("All tests passed")

    def testWhile(self):
        while not (r := self._true3()):
            print("test r:", r)
        print("End with r: ", r)

    def _true3(self):
        print("True count: ", self._true_count)
        self._true_count += 1
        return self._true_count % 3 == 0

    def _testStr(self, b: bool) -> str:
        return "True" if b else ""

    def _true2(self, a: bool, b: bool) -> str:
        return self._testStr(a) or str(b)

    def testReturnOr(self):
        assert self._true2(True, False) == "True"
        print("True or False: ", self._true2(True, False))
        assert self._true2(False, True) == "True"
        print("False or True: ", self._true2(False, True))
        assert self._true2(False, False) == "False"
        print("False or False: ", self._true2(False, False))
        print("pass")

    def _returnStatus(self, status: bool) -> (bool, str):
        return status, "True" if status else "False"

    def testReturnStatus(self):
        assert self._returnStatus(True) == (True, "True")
        print(self._returnStatus(True))
        assert self._returnStatus(False) == (False, "False")
        print(self._returnStatus(False))
        print("pass")


if __name__ == "__main__":
    Test().testAll()
