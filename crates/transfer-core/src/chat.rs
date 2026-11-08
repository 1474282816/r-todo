//! 会话式互传的数据模型（平台无关）。
//!
//! 存储落在壳层（桌面 SQLite / 手机 Drift），本 crate 只负责：
//! - 定义跨端一致的消息形状（事件契约 v2）
//! - 把自定义端点收到的 JSON 解析成 [`ChatMessage`]
//! - 提供「首次确认」信任回执与图片缩略图的判定辅助

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// base64 缩略图上限（Q24：长边 ≤320px、≤64KB）。
pub const MAX_PREVIEW_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageKind {
    Text,
    Image,
    File,
    /// 双向免确认回执：接收方确认首次文件后回给发送方。
    Trust,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageStatus {
    Pending,
    Sent,
    Failed,
}

/// 一条会话消息。`conversation_id` 在本地库视角就是**对端 fingerprint**（Q8）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ChatMessage {
    pub id: String,
    pub conversation_id: String,
    /// 发送方 fingerprint；接收侧由 mTLS 证书覆盖，不可伪造。
    pub sender: String,
    pub sender_alias: String,
    pub kind: MessageKind,
    /// 文本正文，或文件/图片的文件名。
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_size: Option<u64>,
    /// 图片缩略图 base64（随消息到达，落盘前即可预览）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    /// 转发标记：「已转发给 X」。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forwarded_from: Option<String>,
    pub created_at: i64,
    pub status: MessageStatus,
}

impl ChatMessage {
    pub fn new(
        id: impl Into<String>,
        conversation_id: impl Into<String>,
        sender: impl Into<String>,
        sender_alias: impl Into<String>,
        kind: MessageKind,
        body: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            conversation_id: conversation_id.into(),
            sender: sender.into(),
            sender_alias: sender_alias.into(),
            kind,
            body: body.into(),
            file_path: None,
            file_size: None,
            preview: None,
            forwarded_from: None,
            created_at: now_millis(),
            status: MessageStatus::Sent,
        }
    }

    pub fn is_trust(&self) -> bool {
        self.kind == MessageKind::Trust
    }

    pub fn preview_too_large(&self) -> bool {
        self.preview.as_ref().is_some_and(|p| p.len() > MAX_PREVIEW_BYTES)
    }

    pub fn from_json_value(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(value)
    }

    /// 发送侧：把本端视角的消息改成对端视角（会话 id 换成发送方自己）。
    pub fn for_peer(&self, self_fingerprint: &str) -> Self {
        let mut out = self.clone();
        out.conversation_id = self_fingerprint.to_string();
        out
    }
}

pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_roundtrips_with_optional_fields() {
        let mut msg = ChatMessage::new("m1", "PEER", "SELF", "PC", MessageKind::Text, "你好");
        msg.file_path = None;
        let json = serde_json::to_string(&msg).unwrap();
        let back: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, msg);
        assert!(!json.contains("preview"));
    }

    #[test]
    fn trust_messages_are_recognized() {
        let msg = ChatMessage::new("m2", "PEER", "SELF", "PC", MessageKind::Trust, "trust");
        assert!(msg.is_trust());
    }

    #[test]
    fn for_peer_swaps_conversation_id() {
        let msg = ChatMessage::new("m3", "PEER", "SELF", "PC", MessageKind::Text, "hi");
        assert_eq!(msg.for_peer("SELF").conversation_id, "SELF");
        assert_eq!(msg.conversation_id, "PEER");
    }

    #[test]
    fn oversized_preview_is_rejected() {
        let mut msg = ChatMessage::new("m4", "PEER", "SELF", "PC", MessageKind::Image, "a.png");
        msg.preview = Some("x".repeat(MAX_PREVIEW_BYTES + 1));
        assert!(msg.preview_too_large());
    }
}
