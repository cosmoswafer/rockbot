from util.logger import logger
import asyncio
import traceback


def defJson(default_value, source_flag=""):
    """
    A decorator factory that creates a decorator to handle specific exceptions in functions that process JSON data.

    When applied to a function, the decorator will catch `KeyError`, `IndexError`, and `TypeError` exceptions
    that typically occur when accessing non-existent keys or indices in JSON data. If such an exception is caught,
    the function logs the exception with a debug and info level, along with an optional source flag for easier
    identification of the context. After logging, it returns a predefined default value.

    Parameters:
    - default_value: The value to return if an exception is caught. This value should be chosen to indicate
      a failed operation in a way that is compatible with the normal return type of the decorated function.
    - source_flag (str, optional): A string that can be used to indicate the source or context where the exception
      occurred. This helps in debugging by providing additional information in the log messages.

    Returns:
    - A decorator function that can be applied to any function that requires exception handling for JSON data processing.

    Usage example:
    ```
    @defJson(default_value=None, source_flag="data_loader")
    def get_data(json_data, key):
        return json_data[key]

    json_data = {"name": "Alice"}
    print(get_data(json_data, "name"))  # Output: Alice
    print(get_data(json_data, "age"))   # Output: None (and logs the exception with the default value)
    ```
    """

    def wrap(f):
        def wrapped_f(*args, **kwargs):
            try:
                return f(*args, **kwargs)
            except (KeyError, IndexError, TypeError) as e:
                logger.debug(f"Exception in {source_flag}")
                logger.info(
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
                    logger.debug(f"Exception: {e}, retrying...")
                    await asyncio.sleep(cooldown_time)
            raise Exception(f"Failed after {times} times of retrying")

        return wrapped_f

    return wrap
