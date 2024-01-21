#!/bin/bash

# Assuming config.json is in the same directory as the script,
# otherwise provide the full path to the config.json file
CONFIG_FILE="config.json"

# Extract the key using jq
OPENAI_API_KEY=$(jq -r '.bot.openai.key' "$CONFIG_FILE")
OPENAI_API_BASE=$(jq -r '.bot.openai.url.base' "$CONFIG_FILE")

# Now you can use $OPENAI_API_KEY variable that contains your API key
#echo "Extracted API Key: $OPENAI_API_KEY"

# Get the input image for editing from the first parameter and check it exists
if [ -z "$1" ] || [ ! -f "$1" ]; then
  echo "No input image specified"
  exit 1
fi
EDITIMG="$1"
echo "Input image: $EDITIMG"
# Get the prompt from the second parameter
if [ -z "$2" ]; then
  echo "No prompt specified"
  exit 1
fi
PROMPT="$2"
echo "Prompt: $PROMPT"

echo

curl $OPENAI_API_BASE/v1/images/generations \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -d '{
    "model": "dall-e-3",
    "prompt": "A cute baby sea otter",
    "n": 1,
    "size": "1024x1024"
  }'

echo curl $OPENAI_API_BASE/v1/images/edits \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -F image="@$EDITIMG" \
  -F prompt="$PROMPT" \
  -F model="dall-e-2" \
  -F n=1 \
  -F size="1024x1024"

  #-F mask="@mask.png" \
