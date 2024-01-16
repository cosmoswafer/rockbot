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


def retryStrA0(times, s):
    def wrap(f):
        async def wrapped_f(*args, **kwargs):
            try:
                return await f(*args, **kwargs)
            except Exception as e:
                print(f"Exception: {e}, return {s}")
                return s

        return wrapped_f

    return wrap


def retryStrA(times=3, sleep=5, default_str=""):
    """
    Attempt to execute the function for several times, if failed, return the exception message as a string
    """

    def wrap(f):
        async def wrapped_f(*args, **kwargs):
            for i in range(times):
                try:
                    return await f(*args, **kwargs)
                except Exception as e:
                    if i == times - 1:
                        return (
                            str(e)
                            or default_str
                            or f"Failed after {times} times of retrying"
                        )
                    else:
                        print(f"Exception: {e}, retrying...")
                        await asyncio.sleep(sleep)

        return wrapped_f

    return wrap
