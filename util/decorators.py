import asyncio


def defJson(default_value={}):
    def wrap(f):
        def wrapped_f(*args, **kwargs):
            try:
                return f(*args, **kwargs)
            except (KeyError, IndexError) as e:
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
                    await asyncio.sleep(1)
            raise Exception(f"Failed after {times} times of retrying")

        return wrapped_f

    return wrap
