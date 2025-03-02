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
import base64
from datetime import datetime
from typing import Any, Dict, Generator, Iterator, List, Union, Callable
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
        REPLICATE_MODEL_NAME: str = Field(
            default="black-forest-labs/flux-1.1-pro",
            description="Replicate Model prediction name",
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
            REPLICATE_MODEL_NAME=os.getenv(
                "REPLICATE_MODEL_NAME",
                "black-forest-labs/flux-1.1-pro",
            ),
        )


    async def _image_generations(self, headers: Dict[str, str], payload: Dict[str, Any]) -> str:
        err_json = {}
        try:
            # Create prediction
            response = requests.post(
                url=self.valves.REPLICATE_MODEL_NAME_URL,
                headers=headers,
                json=payload,
                timeout=(3.05, 60),
            )
            response.raise_for_status()
            prediction = response.json()
            err_json["create_prediction"] = prediction

            # Poll for completion
            prediction_id = prediction["id"]
            while prediction["status"] not in ["succeeded", "failed", "canceled"]:
                poll_url = f"{self.valves.REPLICATE_API_BASE_URL}/{prediction_id}"
                print(f"Polling {poll_url}")
                response = requests.get(poll_url, headers=headers)
                prediction = response.json()
                print(f"Prediction status: {prediction}")
                if prediction["status"] == "failed":
                    return f"Error: Generation failed: {prediction.get('error', 'Unknown error')}"
            err_json["poll_prediction"] = prediction

            # Handle the completed prediction
            if prediction["status"] == "succeeded":
                # Replicate returns a URL directly
                image_url = prediction["output"]
                # Download the image and convert to base64
                img_response = requests.get(image_url)
                img_response.raise_for_status()
                content_type = img_response.headers.get("Content-Type", "image/jpeg")
                image_base64 = base64.b64encode(img_response.content).decode("utf-8")
                return f"![Image](data:{content_type};base64,{image_base64})\n`GeneratedImage.{content_type.split('/')[-1]}`"
                #return f"![Image]({image_url})"
            err_json["completed_prediction"] = img_response

            return "Error: Image generation failed" + err_json

        except requests.exceptions.RequestException as e:
            return f"Error: Request failed: {e}"
        except Exception as e:
            return f"Error: {e} err_json: {err_json}"

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

        headers = {
            "Authorization": f"Bearer {self.valves.REPLICATE_API_TOKEN}",
            "Content-Type": "application/json",
        }
        payload = {
            "input": {
                "prompt": prompt,
                "aspect_ratio": "1:1",
                "output_format": "png",
                "safety_tolerance": 6,
            },
        }
        try:
            image = await self._image_generations(headers, payload)
            await __event_emitter__(
                {
                    "type": "status",
                    "data": {"description": "Generated an image", "done": True},
                }
            )

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
