import json
import asyncio
import aiohttp
from urllib.parse import urljoin
from util.config import openai as conf
from util.decorators import defJson, retryA, retryStrA


class ApiClient:
    chat_completions_api = urljoin(conf.url.base, conf.url.chat)
    draw_images_api = urljoin(conf.url.base, conf.url.draw)
    headers = {
        "Content-Type": "application/json",
        "Authorization": f"Bearer {conf.key}",
    }
    postdata = {"model": conf.model, "messages": []}
    msg_template = {"role": "user", "content": ""}
    rep_template = {"role": "assistant", "content": ""}
    draw_data = {
        "model": "dall-e-3",
        "prompt": "a photo of a happy corgi puppy sitting and facing forward, studio light, longshot",
        "n": 1,
        "size": "1024x1024",
    }

    @retryStrA(5, 3, "Failed to post data to openai, please check application log")
    async def apiPost(self, url: str, data: dict) -> dict:
        # print("Sending the following request to openai:", data)
        async with aiohttp.ClientSession(headers=self.headers) as s:
            async with s.post(url, json=data) as response:
                if response.status != 200:
                    if conf.debug:
                        print(f"Response body: {await response.text()}")
                    raise Exception(
                        f"Failed to post data to openai, status code: {response.status} \n Response body: {await response.text()}"
                    )
                else:
                    return await response.json()


class OpenAi(ApiClient):
    tools = [
        {
            "type": "function",
            "function": {
                "name": "get_current_weather",
                "description": "Get the current weather",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "location": {
                            "type": "string",
                            "description": "The city and state, e.g. San Francisco, CA",
                        },
                    },
                    "required": ["location"],
                },
            },
        },
        {
            "type": "function",
            "function": {
                "name": "draw",
                "description": "Draw a image using OpenAI's DALL-E 3 model with the given prompt",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "prompt": {
                            "type": "string",
                            "description": "The prompt to draw the image",
                        },
                    },
                    "required": ["prompt"],
                },
            },
        },
    ]

    histories = {}
    # ONLY stores messages not the whole playload, in a list for each user id
    # TODO implemente the presistence history class

    def _compose_message(self, message, history=[]):
        pd = {**self.postdata}

        if OpenAi.tools:
            pd["tools"] = OpenAi.tools

        pd["messages"] = [*history, {**self.msg_template, "content": message}]
        return pd

    def _compose_reply(self, reply):
        pass

    @defJson("")
    def _patch_reply(self, reply_content, role="assistant"):
        # return reply["choices"][0]["message"]
        # reply_content = reply["choices"][0]["message"]
        if reply_content["role"] != role:
            # Patch the role
            reply_content["role"] = role
        return reply_content

    """
    @retryA(5)
    async def _post(self, url: str, data: dict) -> dict:
        # print("Sending the following request to openai:", data)
        async with aiohttp.ClientSession(headers=self.headers) as s:
            async with s.post(url, json=data) as response:
                if response.status != 200:
                    if conf.debug:
                        print(f"Response body: {await response.text()}")
                    raise Exception(
                        f"Failed to post data to openai, status code: {response.status}"
                    )
                else:
                    return await response.json()
    """

    @defJson("")
    def _parse_draw_images(self, data: dict) -> str:
        assert "created" in data and "data" in data, "Invalid response from openai"
        return str(data["data"])

    @defJson("")
    def _parse_message(self, message, content_only=True):
        return (
            message["choices"][0]["message"]["content"]
            if content_only
            else message["choices"][0]["message"]
        )

    def _parseMessageWithErrors(self, message: dict):
        return self._parse_message(message) or str(message)

    async def _cleanup_history(self, user_id):
        # Check maximum history size
        if (
            user_id in OpenAi.histories
            and len(OpenAi.histories[user_id]) > conf.max_history_size
        ):
            if conf.debug:
                print("chatBot: Removing the old messages")
            # Strip the oldest message and keek the latest ten messages
            OpenAi.histories[user_id] = OpenAi.histories[user_id][
                -1 * conf.max_history_size:
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
                if conf.debug:
                    print("chatBot: Removing the oldest message")
                OpenAi.histories[user_id].pop()

    @defJson("")
    async def _parseImageFromDraw(self, data):
        return data["data"][0]["url"]

    async def draw(self, prompt):
        r = await self.apiPost(
            self.draw_images_api, self.draw_data | {"prompt": prompt}
        )
        # return await self._parseImageFromDraw(r)
        return r

    async def _postMessagesWithFunctions(self, messages: dict, user_id: str) -> str:
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

        # r = self._parse_message(await self._post(self.chat_completions_api, messages))
        # while not (r := self._post(self.chat_completions_api, messages)):
        r = await self.apiPost(self.chat_completions_api, messages)
        t = self._parse_message(r, content_only=False)
        print("Check tool calls", t["tool_calls"] if "tool_calls" in t else t)
        function_call_messages = []
        if not t or "tool_calls" not in t:
            return r

        for call in t["tool_calls"]:
            if call["function"]["name"] == "draw":
                # function_results = await self.draw(call["parameters"]["prompt"])
                # print("call", call)

                # Parse the function arguments
                """
                print("call function", call["function"]["name"])
                print("call arguments", call["function"]["arguments"])
                print("call id", call["id"])
                """
                function_name = call["function"]["name"]
                function_arguments = json.loads(call["function"]["arguments"])
                function_id = call["id"]
                print(
                    f'Call function {function_name} with prompt {function_arguments["prompt"]}'
                )
                function_call_messages.append(
                    f'Called function "{function_name}" with prompt "{function_arguments["prompt"]}"'
                )

                # Run the function with the arguments
                fr = await self.draw(function_arguments["prompt"])
                print("function_results", fr)

                # Compose the next message
                messages["messages"].append(
                    self._patch_reply(
                        {
                            **self.rep_template,
                            "content": self._parse_draw_images(fr),
                        },
                        role="tool",
                    )
                )
                # Append the function call result to the histories
                OpenAi.histories[user_id] = [
                    *messages["messages"],
                    {
                        "role": "tool",
                        "content": self._parse_draw_images(fr),
                        "tool_call_id": function_id,
                    },
                ]

        """
        r = await self._post(self.chat_completions_api, messages)
        t = self._parse_message(r, content_only=False)
        """
        return "\n".join(function_call_messages)

    async def submit(self, user_id, message) -> str:
        await self._cleanup_history(user_id)

        h = OpenAi.histories.get(user_id, [])

        m = self._compose_message(message, h)

        """
        if conf.debug:
            print("OpenAi: Sending the following request to openai:", m)
        """
        r = await self._postMessagesWithFunctions(m, user_id)

        """
        if conf.debug:
            print("OpenAi: Received the following response from openai:", r)
        """
        if t := self._parse_message(r):
            OpenAi.histories[user_id] = [*m["messages"], self._patch_reply(r)]
            return t.strip()
        else:
            return str(r)
