from util.logger import logger
import json
import aiohttp
from util.config import openai as conf
from util.decorators import defJson
from util.fluxDraw import FluxDraw
import os


class ApiClient:
    chat_completions_api = f"{conf.url.base.rstrip('/')}/{conf.url.chat.lstrip('/')}"
    draw_images_api = f"{conf.url.base.rstrip('/')}/{conf.url.draw.lstrip('/')}"
    headers = {
        "Content-Type": "application/json",
        "Authorization": f"Bearer {conf.key}",
        "Accept": "application/json",
    }
    postdata = {"model": conf.model, "messages": []}
    msg_template = {"role": "user", "content": ""}
    rep_template = {"role": "assistant", "content": ""}
    draw_data = {
        #"model": "dall-e-3",
        "prompt": "a photo of a happy corgi puppy sitting and facing forward, studio light, longshot",
        "output_format": "png",
        "safety_tolerance": 6
    }

    @defJson("")
    def _parse_stream_delta_content(self, json: dict) -> str:
        return (
            json["choices"][0]["delta"]["content"]
            if "content" in json["choices"][0]["delta"]
            and json["choices"][0]["delta"]["content"] is not None
            else ""
        )

    @defJson([])
    def _parse_stream_delta_toolcalls(self, json: dict) -> list:
        return (
            json["choices"][0]["delta"]["tool_calls"]
            if "tool_calls" in json["choices"][0]["delta"]
            else []
        )

    @defJson({})
    def _parse_stream_delta(self, json: dict) -> dict:
        return json["choices"][0]["delta"]

    @defJson([])
    def _combine_stream_toolcalls(self, toolcalls: list, last_toolcalls: list) -> list:
        return_toolcalls = [*last_toolcalls]
        for tc in toolcalls:
            if tc["index"] == len(return_toolcalls):
                return_toolcalls.append(tc)
                # ASSUMED index values are continuous increasing
            else:
                last_function_data = return_toolcalls[tc["index"]]["function"]
                return_toolcalls[tc["index"]] |= {k: v for k, v in tc.items() if v}
                return_toolcalls[tc["index"]]["function"] = last_function_data | {
                    k: v for k, v in tc["function"].items() if k != "arguments"
                }
                if "arguments" not in return_toolcalls[tc["index"]]["function"]:
                    return_toolcalls[tc["index"]]["function"]["arguments"] = ""
                return_toolcalls[tc["index"]]["function"]["arguments"] += (
                    tc["function"]["arguments"] if "arguments" in tc["function"] else ""
                )
        return return_toolcalls

    @defJson({})
    def _combine_stream_json(self, jsonl: list) -> dict:
        # Combine the content of the stream json from openai stream api into one json
        delta_contents = []
        delta_toolcalls = []
        r = {}
        for j in jsonl:
            logger.debug("Combine stream json:", j)
            delta_contents.append(self._parse_stream_delta_content(j))
            delta_toolcalls = self._combine_stream_toolcalls(
                self._parse_stream_delta_toolcalls(j), delta_toolcalls
            )
            d = self._parse_stream_delta(j)
            last_message = self._parse_message_dict(r) if r else {}
            message = {
                **last_message,
                **{k: v for k, v in d.items() if v},
                "content": "".join(delta_contents),
            }
            if delta_toolcalls:
                message["tool_calls"] = delta_toolcalls
            r = {
                **r,
                **{k: v for k, v in j.items() if v},
                "choices": [{"message": message}],
            }
            logger.debug("Combined stream json:", r)
        return r

    # @retryA(2, 15)
    async def apiPostNetworking(self, url: str, data: dict, use_stream=True) -> dict:
        if use_stream:
            data["stream"] = True

        logger.debug(f"Sending the following request to openai: {data}")

        # Configure proxy if environment variable is set
        proxy = os.environ.get("PROXY")
        if proxy:
            logger.debug(f"Using proxy: {proxy}")

        async with aiohttp.ClientSession(headers=self.headers) as s:
            async with s.post(url, json=data, proxy=proxy) as response:
                if response.status != 200:
                    # TODO Raise exception to retry, or use json response to check the error
                    logger.warning(
                        f"Networking error, status code: {response.status} Response body: {await response.text()}"
                    )
                    return {
                        "type": "error",
                        "data": await response.text(),
                        "status": response.status,
                        "message": "Failed to post data to openai",
                    }
                elif response.content_type == "text/event-stream":
                    r = []
                    async for line in response.content:
                        logger.debug(f"Stream line: {line}")
                        if line.startswith(b"data:") and line[6:].strip() != b"[DONE]":
                            logger.debug(f"Data line: {line[6:]}")
                            r.append(json.loads(line[6:]))
                    return {"type": "stream", "data": r}
                elif response.content_type == "application/json":
                    return await response.json()
                else:
                    return await self._compose_response_from_text(response.text())

    async def apiPost(self, url: str, data: dict, use_stream=True) -> dict:
        r = await self.apiPostNetworking(url, data, use_stream)
        if "type" in r and r["type"] == "stream":
            return self._combine_stream_json(r["data"])
        else:
            return r


class OpenAi(ApiClient):
    flux = FluxDraw.FromConfig(conf.draw)
    tools = [
        {
            "type": "function",
            "function": {
                "name": "draw",
                "description": "Draw an image using Flux 1.1 pro ultra model with the given prompt",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "prompt": {
                            "type": "string",
                            "description": "The prompt to draw the image",
                        },
                        "aspect_ratio": {
                            "type": "string",
                            "description": "The aspect ratio of the image that will be generated. Must be one of `1:1`(square), `2:3`(portrait), `3:2`(landscape), `9:16`(portrait) or `16:9`(landscape).",
                        },
                        "required": ["prompt"],
                    },
                },
            },
        },
    ]

    histories = {}
    # ONLY stores messages not the whole playload, in a list for each user id
    # TODO implemente the presistence history class

    def _compose_message(self, message, history=[]):
        pd = {**self.postdata} | {"model": conf.model}
        logger.debug(f"Using model: {pd['model']}")

        if OpenAi.tools and conf.tools:
            pd["tools"] = OpenAi.tools

        pd["messages"] = [*history, {**self.msg_template, "content": message}]
        return pd

    def _compose_reply(self, reply):
        pass

    def _patch_reply_role(self, reply_content, role="assistant"):
        if reply_content["role"] != role:
            # Patch the role
            reply_content["role"] = role
        return reply_content

    @defJson("")
    def _parse_draw_images(self, data: dict) -> str:
        assert "created" in data and "data" in data, "Invalid response from openai"
        return str(data["data"])

    def _compose_response_from_text(self, text):
        return {"choices": [{"message": {"role": "dummy", "content": text}}]}

    @defJson("")
    def _parse_message(self, message) -> str:
        return self._parse_message_dict(message)["content"]

    @defJson({})
    def _parse_message_dict(self, message) -> dict:
        return message["choices"][0]["message"]

    def _parse_message_with_errors(self, message: dict):
        return self._parse_message(message) or str(message)

    async def _cleanup_history(self, user_id):
        # Check maximum history size
        if (
            user_id in OpenAi.histories
            and len(OpenAi.histories[user_id]) > conf.max_history_size
        ):
            logger.debug("chatBot: Removing the old messages")
            # Strip the oldest message and keek the latest ten messages
            OpenAi.histories[user_id] = OpenAi.histories[user_id][
                -1 * conf.max_history_size :
            ]

        # Check maximum text length
        if user_id in OpenAi.histories:
            """
            while (
                len("".join([m["content"] for m in OpenAi.histories[user_id]]))
                > conf.max_text_length
            ):
            """
            while len(str(OpenAi.histories[user_id])) > conf.max_text_length:
                logger.debug("chatBot: Removing the oldest message")
                OpenAi.histories[user_id].pop()

    @defJson("")
    def _parse_draw_function_result(self, json_data: dict) -> str:
        revised_prompt = json_data["data"][0]["revised_prompt"]
        image_url = json_data["data"][0]["url"]
        return f"Image url: [{revised_prompt}]({image_url})"
        # return f"![{revised_prompt}]({image_url})"

    async def draw_v_openai(self, prompt: str, aspect_ratio: str) -> dict:
        aspect_ratio_options = ["1:1", "2:3", "3:2", "9:16", "16:9"]
        r = await self.apiPost(
            self.draw_images_api,
            self.draw_data
            | {
                "prompt": prompt,
                "aspect_ratio": aspect_ratio
                if aspect_ratio in aspect_ratio_options
                else aspect_ratio_options[0],
            },
            False,
        )
        return r
    
    async def draw(self, prompt: str, aspect_ratio: str) -> str:
        aspect_ratio_options = ["1:1", "2:3", "3:2", "9:16", "16:9"]
        r = await self.flux.drawApi(prompt, aspect_ratio)
        return r


    async def postMessagesWithFunctions(self, messages: dict, user_id: str) -> dict:
        """
        example_calls = {
            "id": "chatcmpl-8TbC4VvqbExdYtZabYbsS8VBuWRuk",
            "object": "chat.completion",
            "created": 1702065176,
            "model": "gpt-4-1106-preview",
            "choices ": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": None,
                        "tool_calls": [
                            {
                                "id": "call_vTbb5TvamnPslOB5Gc0XVxoR",
                                "type": "function",
                                "function": {
                                    "name": "draw",
                                    "arguments": '{"prompt":"cat"}',
                                },
                            }
                        ],
                    },
                    "finish_reason": "tool_calls",
                }
            ],
            "usage": {
                "prompt_tokens": 113,
                "comple tion_tokens": 13,
                "total_tokens": 126,
            },
            "system_fingerprint": "fp_a24b4d720c",
        }
        tool_calls_example = [
            {
                "id": "call_vTbb5TvamnPslOB5Gc0XVxoR",
                "type": "function",
                "function": {"name": "draw", "arguments": '{"prompt":"cat"}'},
            }
        ]
        tool_calls_messages = {
            "role": "tool",
            "content": "http://xxxxxxxxxxxx.png",
            "tool_call_id": "call_vTbb5TvamnPslOB5Gc0XVxoR",
        }
        """

        r = await self.apiPost(self.chat_completions_api, messages)
        t = self._parse_message_dict(r)
        logger.debug(f'Check tool calls {t["tool_calls"] if "tool_calls" in t else t}')
        function_call_messages = []
        if not t or "tool_calls" not in t:
            return r

        for call in t["tool_calls"]:
            # ASUMED tools calls has all valid arguments
            if call["function"]["name"] == "draw":
                # function_results = await self.draw(call["parameters"]["prompt"])
                # print("call", call)

                # Parse the function arguments
                function_name = call["function"]["name"]
                function_arguments = json.loads(call["function"]["arguments"])
                function_id = call["id"]
                logger.debug(
                    f"Call function {function_name} with arguments {function_arguments}"
                )
                # function_call_messages.append(
                #    f'Called function "{function_name}" with prompt "{function_arguments["prompt"]}"'
                # )

                # Run the function with the arguments
                fr = await self.draw(
                    function_arguments["prompt"],
                    function_arguments["aspect_ratio"] if "aspect_ratio" in function_arguments else "",
                )
                logger.debug(f"function_results {fr}")
                # function_call_messages.append(
                #    f'The following is your results, plaes save it manually: "{fr["data"][0]["url"]}"'
                # )
                """
                # TODO Convert into markdown syntax
                if t := self._parse_draw_function_result(fr):
                    function_call_messages.append(t)
                    function_call_messages.append(
                        "The image URL provided above is available for only a few minutes. Please save it manually."
                    )
                else:
                    function_call_messages.append(str(fr))
                    function_call_messages.append("Error on function calling.")
                """
                function_call_messages.append(fr)
            else:
                function_call_messages.append(
                    f'Unknown function "{call["function"]["name"]}" with arguments "{call["function"]["arguments"]}"'
                )

        return self._compose_response_from_text("\n".join(function_call_messages))

    async def submit(self, user_id, message) -> str:
        await self._cleanup_history(user_id)

        h = OpenAi.histories.get(user_id, [])

        post_msg = self._compose_message(message, h)

        r = await self.postMessagesWithFunctions(post_msg, user_id)

        if t := self._parse_message(r):
            OpenAi.histories[user_id] = [
                *post_msg["messages"],
                self._patch_reply_role(self._parse_message_dict(r)),
            ]
            return t.strip()
        else:
            return str(r)
