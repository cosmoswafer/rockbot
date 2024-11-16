"""
title: Image Gen
author: open-webui
author_url: https://github.com/open-webui
funding_url: https://github.com/open-webui
version: 0.1
required_open_webui_version: 0.3.9
"""

import os
import requests
from datetime import datetime
from typing import Callable
from pydantic import BaseModel, Field

#from open_webui.apps.images.main import image_generations, GenerateImageForm
from open_webui.apps.webui.models.users import Users


class Tools:

    class Valves(BaseModel):
        """
        Pydantic model for storing API keys and base URLs.
        """

        REPLICATE_API_TOKEN: str = Field(
            default="", description="Your API Token for Replicate"
        )
        REPLICATE_API_BASE_URL: str = Field(
            default="https://api.replicate.com/v1/predictions",
            description="Base URL for the Replicate API",
        )
        REPLICATE_MODEL_NAME_URL: str = Field(
            default="https://api.replicate.com/v1/models/black-forest-labs/flux-1.1-pro/predictions",
            description="Replicate Model prediction API url",
        )

    def __init__(self):
        """
        Initialize the Pipe class with default values and environment variables.
        """
        self.type = "manifold"
        self.id = "FLUX_1_1_PRO"
        self.name = "FLUX.1.1-pro: "
        self.valves = self.Valves(
            REPLICATE_API_TOKEN=os.getenv("REPLICATE_API_TOKEN", ""),
            REPLICATE_API_BASE_URL=os.getenv(
                "REPLICATE_API_BASE_URL",
                "https://api.replicate.com/v1/predictions",
            ),
            REPLICATE_MODEL_NAME_URL=os.getenv(
                "REPLICATE_MODEL_NAME_URL",
                "https://api.replicate.com/v1/models/black-forest-labs/flux-1.1-pro/predictions",
            ),
        )
    def __init__(self):
        pass

    async def generate_image(
        self, prompt: str, __user__: dict, __event_emitter__=None
    ) -> str:
        """
        Generate an image given a prompt

        :param prompt: prompt to use for image generation
        """

        await __event_emitter__(
            {
                "type": "status",
                "data": {"description": "Generating an image", "done": False},
            }
        )

        try:
            images = await image_generations(
                GenerateImageForm(**{"prompt": prompt}),
                Users.get_user_by_id(__user__["id"]),
            )
            await __event_emitter__(
                {
                    "type": "status",
                    "data": {"description": "Generated an image", "done": True},
                }
            )

            for image in images:
                await __event_emitter__(
                    {
                        "type": "message",
                        "data": {"content": f"![Generated Image]({image['url']})"},
                    }
                )

            return f"Notify the user that the image has been successfully generated"

        except Exception as e:
            await __event_emitter__(
                {
                    "type": "status",
                    "data": {"description": f"An error occured: {e}", "done": True},
                }
            )

            return f"Tell the user: {e}"
