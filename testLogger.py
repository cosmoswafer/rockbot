from base.utils import logger as logger
from base.utils import openai_config as config

logger.info("Testing the logger")
logger.info(f"Loaded configuration: {config}")
