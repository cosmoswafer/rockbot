use serde::{Deserialize, Serialize};

use crate::error::{Result, RockBotError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    #[serde(rename = "tool_calls")]
    ToolUse,
    #[serde(rename = "content_filter")]
    ContentFilter,
    #[serde(rename = "insufficient_system_resource")]
    InsufficientSystemResource,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Multipart(Vec<ContentPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text {
        text: String,
    },
    #[serde(rename = "image_url")]
    ImageUrl {
        image_url: ImageUrlPayload,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageUrlPayload {
    pub url: String,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: MessageContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: MessageContent::Text(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Text(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    pub fn user_with_image(text: impl Into<String>, image_data_uri: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Multipart(vec![
                ContentPart::Text {
                    text: text.into(),
                },
                ContentPart::ImageUrl {
                    image_url: ImageUrlPayload {
                        url: image_data_uri.into(),
                        detail: Some("high".into()),
                    },
                },
            ]),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    pub fn user_with_image_url(
        text: impl Into<String>,
        image_url: impl Into<String>,
    ) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Multipart(vec![
                ContentPart::Text {
                    text: text.into(),
                },
                ContentPart::ImageUrl {
                    image_url: ImageUrlPayload {
                        url: image_url.into(),
                        detail: Some("high".into()),
                    },
                },
            ]),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    pub fn user_with_images(
        text: impl Into<String>,
        image_data_uris: Vec<String>,
    ) -> Self {
        let mut parts: Vec<ContentPart> = Vec::with_capacity(image_data_uris.len() + 1);
        parts.push(ContentPart::Text {
            text: text.into(),
        });
        for uri in image_data_uris {
            parts.push(ContentPart::ImageUrl {
                image_url: ImageUrlPayload {
                    url: uri,
                    detail: Some("high".into()),
                },
            });
        }
        Self {
            role: Role::User,
            content: MessageContent::Multipart(parts),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: MessageContent::Text(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    pub fn assistant_with_tool_calls(
        content: impl Into<String>,
        tool_calls: Vec<ToolCall>,
        reasoning_content: Option<String>,
    ) -> Self {
        Self {
            role: Role::Assistant,
            content: MessageContent::Text(content.into()),
            name: None,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            reasoning_content,
        }
    }

    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: MessageContent::Text(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            reasoning_content: None,
        }
    }

    pub fn text_content(&self) -> Option<&str> {
        match &self.content {
            MessageContent::Text(t) => Some(t.as_str()),
            MessageContent::Multipart(_) => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default = "default_tool_type")]
    pub call_type: String,
    pub function: FunctionCall,
}

fn default_tool_type() -> String {
    "function".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

impl ToolCall {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: name.into(),
                arguments: arguments.into(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolDef {
    #[serde(rename = "type", default = "default_tool_type")]
    pub tool_type: String,
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionDef {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub strict: Option<bool>,
}

impl ToolDef {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            tool_type: "function".into(),
            function: FunctionDef {
                name: name.into(),
                description: Some(description.into()),
                parameters: Some(parameters),
                strict: None,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDef>>,
    #[serde(skip_serializing_if = "is_false", default)]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
}

fn is_false(b: &bool) -> bool {
    !b
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThinkingConfig {
    #[serde(rename = "type")]
    pub thinking_type: String,
}

impl ThinkingConfig {
    pub fn enabled() -> Self {
        Self {
            thinking_type: "enabled".into(),
        }
    }

    pub fn disabled() -> Self {
        Self {
            thinking_type: "disabled".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompletionResult {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    pub finish: FinishReason,
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub usage: Option<UsageInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UsageInfo {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone)]
pub enum ImageSizeValue {
    Preset(String),
    Custom { width: u32, height: u32 },
}

#[derive(Debug, Clone)]
pub struct ImageGenParams {
    pub prompt: String,
    pub quality: Option<String>,
    pub image_size: Option<ImageSizeValue>,
    pub size_tier: Option<String>,
    pub output_format: Option<String>,
    pub num_images: Option<u32>,
    pub model_id: Option<String>,
    pub image_urls: Option<Vec<String>>,
}

impl ImageGenParams {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            quality: None,
            image_size: None,
            size_tier: None,
            output_format: None,
            num_images: None,
            model_id: None,
            image_urls: None,
        }
    }

    pub fn resolve_image_size(&self) -> Option<serde_json::Value> {
        match &self.image_size {
            Some(ImageSizeValue::Preset(name)) => Self::lookup_preset(name)
                .map(|(w, h)| serde_json::json!({ "width": w, "height": h }))
                .or_else(|| Some(serde_json::json!(name))),
            Some(ImageSizeValue::Custom { width, height }) => {
                Some(serde_json::json!({ "width": width, "height": height }))
            }
            None => None,
        }
    }

    fn lookup_preset(name: &str) -> Option<(u32, u32)> {
        match name {
            "square_hd" | "1:1" => Some((2880, 2880)),
            "landscape_16_9" | "16:9" => Some((3840, 2160)),
            "portrait_16_9" | "9:16" => Some((2160, 3840)),
            "landscape_4_3" | "4:3" => Some((3312, 2480)),
            "portrait_4_3" | "3:4" => Some((2480, 3312)),
            "landscape_3_2" | "3:2" => Some((3504, 2336)),
            "portrait_2_3" | "2:3" => Some((2336, 3504)),
            "square" => Some((512, 512)),
            _ => None,
        }
    }

    pub fn validate_dimensions(&self) -> Result<()> {
        if let Some(ImageSizeValue::Custom { width, height }) = &self.image_size {
            let pixels = (*width as u64) * (*height as u64);
            let max_edge = std::cmp::max(*width, *height);
            let min_edge = std::cmp::min(*width, *height);

            if max_edge > 3840 {
                return Err(RockBotError::Provider(format!(
                    "image_size max edge must be ≤3840px (got {}×{}).",
                    width, height
                )));
            }

            if min_edge > 0 && max_edge as f64 / min_edge as f64 > 3.0 {
                return Err(RockBotError::Provider(format!(
                    "image_size aspect ratio must be ≤3:1 (got {}×{}).",
                    width, height
                )));
            }

            if !(655_360..=8_294_400).contains(&pixels) {
                return Err(RockBotError::Provider(format!(
                    "image_size pixel count must be 655,360–8,294,400 (got {} from {}×{}).",
                    pixels, width, height
                )));
            }

            if *width % 16 != 0 || *height % 16 != 0 {
                return Err(RockBotError::Provider(format!(
                    "image_size dimensions must be multiples of 16 (got {}×{}).",
                    width, height
                )));
            }
        }
        Ok(())
    }
}
