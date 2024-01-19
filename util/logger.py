import logging
import os

_log_level_options = {
    "INFO": logging.INFO,
    "DEBUG": logging.DEBUG,
    "WARNING": logging.WARNING,
    "ERROR": logging.ERROR,
}
_log_level = os.environ.get("LOG_LEVEL", "INFO").upper()
if _log_level not in _log_level_options:
    # raise ValueError(f"Invalid log level {_log_level}")
    _log_level_value = _log_level_options["INFO"]
else:
    _log_level_value = _log_level_options[_log_level]
logging.basicConfig(
    level=_log_level_value, format="%(asctime)s %(levelname)s %(message)s"
)
logger = logging.getLogger(__name__)
logger.info(f"Initalized logger with LOG_LEVEL={_log_level}")
if _log_level != os.environ.get("LOG_LEVEL", "INFO").upper():
    logger.warning(
        f"Invalid log level {_log_level}, using default log level *INFO* instead"
    )
