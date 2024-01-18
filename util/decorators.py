import asyncio
import traceback


def defJson(default_value={}, source_flag=""):
    def wrap(f):
        def wrapped_f(*args, **kwargs):
            try:
                return f(*args, **kwargs)
            except (KeyError, IndexError, TypeError) as e:
                if source_flag:
                    print("Exception in", source_flag)
                print(
                    f"No such item(s) in json data, return the default value: {default_value}, Exception: {e}"
                )
                return default_value

        return wrapped_f

    return wrap


def retryA(times=3, cooldown_time=5):
    """
    Decorator that retries the decorated async function a specified number of times
    with a cooldown time between retries.

    Args:
        times (int): The number of times to retry the decorated function. Default is 3.
        cooldown_time (int): The cooldown time in seconds between retries. Default is 5.

    Returns:
        function: The decorated async function.

    Raises:
        Exception: If the decorated function fails after the specified number of retries.
    """

    def wrap(f):
        async def wrapped_f(*args, **kwargs):
            for _ in range(times):
                try:
                    return await f(*args, **kwargs)
                except Exception as e:
                    print(f"Exception: {e}, retrying...")
                    await asyncio.sleep(cooldown_time)
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

    Args:
        times (int): The number of times to retry the function execution. Default is 3.
        sleep (int): The number of seconds to sleep between retries. Default is 5.
        default_str (str): The default string to return if all retries fail. Default is an empty string.

    Returns:
        str: The exception message as a string if all retries fail, or the result of the function execution.

    """

    def wrap(f):
        async def wrapped_f(*args, **kwargs):
            for i in range(times):
                try:
                    return await f(*args, **kwargs)
                except Exception as e:
                    if i == times - 1:
                        traceback.print_exc()
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
