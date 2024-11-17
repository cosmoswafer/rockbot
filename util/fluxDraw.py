from typing import Any, Dict
import aiohttp
import base64

class FluxDraw:
    input_def_args = {
       "aspect_ratio": "1:1",
       "output_format": "png",
       "safety_tolerance": 6
    }

    def __init__(self, api_token: str, api_base_url: str, model_name_url: str):
        self.api_token = api_token
        self.api_base_url = api_base_url
        self.model_name_url = model_name_url

        self.headers = {
            "Authorization": f"Bearer {self.api_token}",
            "Content-Type": "application/json",
        }

    async def drawApi(self, payload: Dict[str, Any]) -> str:
        err_json = {}

        prediction_id = ""
        async with aiohttp.ClientSession(headers=self.headers) as s:
            async with s.post(self.model_name_url, json=payload) as response:
                json_response = await response.json()
                err_json["create_prediction"] = json_response
                prediction_id = json_response["id"] if "id" in json_response else ""

        prediction = {"status": "initial", "output": "https://example.com/image.png"}
        while prediction["status"] not in ["succeeded", "failed", "canceled"]:
            async with aiohttp.ClientSession(headers=self.headers) as s:
                async with s.get(f"{self.api_base_url}/{prediction_id}") as response:
                    json_response = await response.json()
                    prediction = json_response
                    if prediction["status"] == "failed":
                        return f"Error: Generation failed: {prediction.get('error', 'Unknown error')}"
        err_json["poll_prediction"] = prediction

        if prediction["status"] == "succeeded":
            image_url = prediction["output"]
            return await self._fetch_img(image_url)
        else:
            return f"Error: {err_json}"

    async def _fetch_img(self, url: str) -> str:
        async with aiohttp.ClientSession(headers=self.headers) as s:
            async with s.get(url) as response:
                content_type = response.headers.get("content-type", "image/png")
                image_base64 = base64.b64encode(await response.read()).decode("utf-8")
                return f"![Image](data:{content_type};base64,{image_base64})\n`GeneratedImage.{content_type.split('/')[-1]}`"
    
    @staticmethod
    def FromConfig(config):
        return FluxDraw(config.api_key, config.base_url, config.model_url)

    
