import logging
import os

_log_level = os.environ.get("LOG_LEVEL", "INFO").upper()
logging.basicConfig(level=_log_level, format="%(asctime)s %(levelname)s %(message)s")
logger = logging.getLogger(__name__)
logger.debug(f"Initalized logger with log level {_log_level}")
