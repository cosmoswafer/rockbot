"""
title: FLUX.1.1 Pro Manifold Function for Black Forest Lab Image Generation Models
author: mobilestack, credit to bgeneto
author_url: https://github.com/mobilestack/open-webui-flux-1.1-pro
funding_url: https://github.com/open-webui
version: 0.3
license: MIT
requirements: pydantic, requests
environment_variables: REPLICATE_API_TOKEN
supported providers: replicate.com
"""

import base64
import os
from typing import Any, Dict, Generator, Iterator, List, Union
import requests
from open_webui.utils.misc import get_last_user_message
from pydantic import BaseModel, Field


class Pipe:
    """
    Class representing the FLUX.1.1-pro Manifold Function.
    """

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

    # [Previous helper methods remain the same]

    def non_stream_response(
        self, headers: Dict[str, str], payload: Dict[str, Any]
    ) -> str:
        """
        Get a non-streaming response from the API.
        """
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
                # img_response = requests.get(image_url)
                # img_response.raise_for_status()
                # content_type = img_response.headers.get("Content-Type", "image/jpeg")
                # image_base64 = base64.b64encode(img_response.content).decode("utf-8")
                # return f"![Image](data:{content_type};base64,{image_base64})\n`GeneratedImage.{content_type.split('/')[-1]}`"
                return f"![Image]({image_url})"
            err_json["completed_prediction"] = img_response

            return "Error: Image generation failed" + err_json

        except requests.exceptions.RequestException as e:
            return f"Error: Request failed: {e}"
        except Exception as e:
            return f"Error: {e} err_json: {err_json}"

    def pipe(
        self, body: Dict[str, Any]
    ) -> Union[str, Generator[str, None, None], Iterator[str]]:
        """
        Process the pipe request.
        """
        headers = {
            "Authorization": f"Bearer {self.valves.REPLICATE_API_TOKEN}",
            "Content-Type": "application/json",
        }

        prompt = get_last_user_message(body["messages"])

        # Replicate-specific payload
        payload = {
            "input": {"prompt": prompt, "prompt_upsampling": True},
        }

        try:
            return self.non_stream_response(headers, payload)
        except requests.exceptions.RequestException as e:
            return f"Error: Request failed: {e}"
        except Exception as e:
            return f"Error: {e}"

    def pipes(self) -> List[Dict[str, str]]:
        """
        Get the list of available pipes.
        """
        return [{"id": "flux_1_1_pro", "name": "Flux 1.1 PRO"}]
