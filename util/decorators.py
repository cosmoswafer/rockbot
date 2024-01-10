import asyncio


def defJson(default_value={}):
    def wrap(f):
        def wrapped_f(*args, **kwargs):
            try:
                return f(*args, **kwargs)
            except (KeyError, IndexError, TypeError) as e:
                print(
                    f"No such item(s) in json data, return the default value: {default_value}, Exception: {e}"
                )
                return default_value

        return wrapped_f

    return wrap


def retryA(times=3):
    def wrap(f):
        async def wrapped_f(*args, **kwargs):
            for i in range(times):
                try:
                    return await f(*args, **kwargs)
                except Exception as e:
                    print(f"Exception: {e}, retrying...")
                    await asyncio.sleep(5)
            raise Exception(f"Failed after {times} times of retrying")

        return wrapped_f

    return wrap


def retryStrA(times, s):
    def wrap(f):
        async def wrapped_f(*args, **kwargs):
            try:
                return await f(*args, **kwargs)
            except Exception as e:
                print(f"Exception: {e}, return {s}")
                return s

        return wrapped_f

    return wrap


def retryStrA(time=3, sleep=5, default_str=""):
    """
    Attempt to execute the function for several times, if failed, return the exception message as a string
    """
    def wrap(f):
        async def wrapped_f(*args, **kwargs):
            for i in range(times):
                exception_message = ""
                try:
                    return await f(*args, **kwargs)
                except Exception as e:
                    print(f"Exception: {e}, retrying...")
                    await asyncio.sleep(sleep)
                    exception_message = str(e)
            return exception_message or default_str or f"Failed after {times} times of retrying"

        return wrapped_f

    return wrap
