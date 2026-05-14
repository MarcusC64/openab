use crate::schema::*;
use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

/// Base URL for Telegram Bot API. Extracted as constant for consistency
/// with LINE's `LINE_API_BASE` and to enable future mock testing.
pub const TELEGRAM_API_BASE: &str = "https://api.telegram.org";

// --- Telegram types ---

#[derive(Debug, Deserialize)]
pub struct TelegramUpdate {
    message: Option<TelegramMessage>,
}

#[derive(Debug, Deserialize)]
struct TelegramMessage {
    message_id: i64,
    message_thread_id: Option<i64>,
    chat: TelegramChat,
    from: Option<TelegramUser>,
    text: Option<String>,
    #[serde(default)]
    entities: Vec<TelegramEntity>,
}

#[derive(Debug, Deserialize)]
struct TelegramEntity {
    #[serde(rename = "type")]
    entity_type: String,
    offset: usize,
    length: usize,
}

#[derive(Debug, Deserialize)]
struct TelegramChat {
    id: i64,
    #[serde(rename = "type")]
    chat_type: String,
    #[allow(dead_code)]
    is_forum: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct TelegramUser {
    id: i64,
    first_name: String,
    last_name: Option<String>,
    username: Option<String>,
    is_bot: bool,
}

// --- Webhook handler ---

pub async fn webhook(
    State(state): State<Arc<crate::AppState>>,
    headers: axum::http::HeaderMap,
    Json(update): Json<TelegramUpdate>,
) -> axum::http::StatusCode {
    if let Some(ref expected) = state.telegram_secret_token {
        let provided = headers
            .get("x-telegram-bot-api-secret-token")
            .and_then(|v| v.to_str().ok());
        if provided != Some(expected.as_str()) {
            warn!("webhook rejected: invalid or missing secret_token");
            return axum::http::StatusCode::UNAUTHORIZED;
        }
    }

    let Some(msg) = update.message else {
        return axum::http::StatusCode::OK;
    };
    let Some(text) = msg.text.as_deref() else {
        return axum::http::StatusCode::OK;
    };
