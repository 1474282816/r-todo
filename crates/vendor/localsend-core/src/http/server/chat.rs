//! r-todo 会话式扩展：自定义消息端点。
//!
//! 这是 vendored LocalSend core 里**唯一的加法改动**（见 `VENDORED.md`）：
//! 只新增两条路由，既有 v2 路径、DTO 与状态机一个都不动，因此官方 LocalSend
//! 客户端的双向互通由构造保证。
//!
//! 消息不进入 v2 的传输会话槽：server 收到 JSON 后原样投递给壳层
//! (`transfer-core`)，由壳层转成 `TransferEvent::Message` 事件。发送方身份取自
//! mTLS 客户端证书 fingerprint，请求体无法伪造。

use std::sync::Mutex;

use hyper::body::Incoming;
use hyper::http::HeaderValue;
use hyper::{header, Request, Response, StatusCode};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::common::collect_to_json::CollectToJson;
use super::common::error::AppError;
use super::common::response::{full_body, BoxedBody};
use super::RequestClientInfo;

/// 壳层注册的消息接收端。服务可能 stop/start 多次，故可替换而不是一次性写入。
static CHAT_SINK: Mutex<Option<mpsc::Sender<Value>>> = Mutex::new(None);

pub fn register_chat_sink(tx: mpsc::Sender<Value>) {
    if let Ok(mut guard) = CHAT_SINK.lock() {
        *guard = Some(tx);
    }
}

fn json_response(value: Value) -> Response<BoxedBody> {
    let mut res = Response::new(full_body(value.to_string()));
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    res
}

/// GET /api/rtodo/v1/capabilities —— 能力探测：404 即官方 LocalSend / 不支持会话。
pub fn capabilities() -> Response<BoxedBody> {
    json_response(json!({ "chat": 1, "schema_version": 2 }))
}

/// POST /api/rtodo/v1/message —— 文本 / 图片元数据 / 信任回执。
pub async fn handle_message(
    req: Request<Incoming>,
    client_info: RequestClientInfo,
) -> Result<Response<BoxedBody>, AppError> {
    let mut value: Value = req.into_body().collect_to_json().await?;

    // 发送方 fingerprint 以 mTLS 证书为准，覆盖请求体里的同名字段。
    let sender = client_info
        .cert_fingerprint()
        .unwrap_or_else(|| client_info.identifier());
    value["sender"] = json!(sender);

    let sink = {
        let guard = CHAT_SINK
            .lock()
            .map_err(|_| AppError::Status(StatusCode::INTERNAL_SERVER_ERROR))?;
        guard.clone()
    }
    .ok_or(AppError::Status(StatusCode::NOT_FOUND))?;
    sink.send(value)
        .await
        .map_err(|_| AppError::Status(StatusCode::SERVICE_UNAVAILABLE))?;

    Ok(json_response(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_advertises_chat_v1() {
        let res = capabilities();
        assert_eq!(res.status(), StatusCode::OK);
    }
}
