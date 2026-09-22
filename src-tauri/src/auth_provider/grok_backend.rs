//! GrokProvider: fixed-URL adapter for xAI Grok CLI chat-proxy.
//!
//! A Grok OAuth account uses one wire profile decided by the route planner:
//!
//! - OpenAI Responses at the fixed `https://cli-chat-proxy.grok.com/v1/responses`
//!   with Grok CLI identity headers and `Authorization: Bearer`
//!
//! The provider performs no protocol conversion (that stays in the codec
//! registry) and never accepts a renderer/downstream-supplied base URL or
//! endpoint.  The trusted `(upstream_protocol, upstream_endpoint)` arrived from
//! the RoutePlan through `ProviderRequest`; anything outside the exact
//! allowlist fails closed before any HTTP request.

use async_trait::async_trait;
use reqwest::header::{
    HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT,
};
use serde_json::Value;

use super::{
    grok_login::{
        GrokLogin, GROK_API_BASE, GROK_AUTHENTICATE_RESPONSE_HEADER,
        GROK_AUTHENTICATE_RESPONSE_VALUE, GROK_CLIENT_IDENTIFIER, GROK_CLIENT_IDENTIFIER_HEADER,
        GROK_CLIENT_VERSION, GROK_CLIENT_VERSION_HEADER, GROK_HTTP_TIMEOUT, GROK_TOKEN_AUTH_HEADER,
        GROK_TOKEN_AUTH_VALUE, GROK_USER_AGENT,
    },
    LoginResult, LoginRuntime, Provider, ProviderError, ProviderKind, ProviderLoginContext,
    ProviderModels, ProviderPayload, ProviderRequest, RefreshedPayload,
};
use crate::db::models::{AuthAccount, ModelState, QuotaState};

const RESPONSES_PATH: &str = "responses";
const MODELS_PATH: &str = "models";

/// Grok cli-chat-proxy 接受的 `tools[].type` 白名单。
///
/// 来源：上游对未知类型的 422 反序列化错误
/// （`unknown variant \`…\`, expected one of \`function\`, \`web_search\`, …`）。
/// 白名单之外的声明（Codex 的 `namespace` / `custom` / `local_shell`、
/// OpenAI 的 `web_search_preview` 等）会让整个请求失败，必须先移除。
const GROK_TOOL_TYPES: &[&str] = &[
    "function",
    "web_search",
    "x_search",
    "image_generation",
    "collections_search",
    "file_search",
    "code_execution",
    "code_interpreter",
    "mcp",
    "shell",
    "tool_search",
];

/// `web_search` 工具内 Grok 上游不接受的字段。
///
/// 实测上游对未知参数返回 400 `Argument not supported: <field>`；
/// `filters` / `user_location` / `indexed_web_access` / `search_content_types`
/// 等字段上游接受，保持原样。
const GROK_UNSUPPORTED_WEB_SEARCH_FIELDS: &[&str] = &["external_web_access", "search_context_size"];

/// Grok 的 reasoning `encrypted_content` 在后续请求里会被上游拒绝。
///
/// 实测：Codex CLI 多轮会话把上一轮的 reasoning `encrypted_content` 原样回传时，
/// 上游返回 400 `Could not decode the compaction blob. Ensure it is unmodified
/// from the compact response.`（同一个 blob 生成后立即回放可以成功，落到真实
/// 会话就失败，说明上游解码依赖短时服务端状态，不能当作可回放上下文）。
/// 因此 Grok 出站不再请求也不转发该字段：只丢弃加密 blob，明文 `summary`
/// 照常保留，模型每轮重新推理。
const GROK_ENCRYPTED_REASONING_INCLUDE: &str = "reasoning.encrypted_content";

fn safe_headers() -> Vec<HeaderName> {
    vec![
        HeaderName::from_static("x-request-id"),
        HeaderName::from_static("traceparent"),
        HeaderName::from_static("tracestate"),
    ]
}

pub struct GrokProvider {
    client: reqwest::Client,
    /// 流式出站客户端（无总超时），见 [`super::streaming_http_client`]。
    stream_client: reqwest::Client,
    api_base: String,
    login: GrokLogin,
}

impl Default for GrokProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl GrokProvider {
    pub fn new() -> Self {
        Self::with_api_base(GROK_API_BASE.to_owned())
    }

    pub fn with_api_base(api_base: impl Into<String>) -> Self {
        Self::with_endpoints(api_base, String::new(), String::new())
    }

    /// 测试专用：把非流式客户端的总超时调到极短，用于验证流式/非流式
    /// 客户端的选择（上游慢响应时，流式必须不受该总超时影响）。
    #[cfg(test)]
    fn with_api_base_and_blocking_timeout(
        api_base: impl Into<String>,
        timeout: std::time::Duration,
    ) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .expect("grok test blocking client"),
            stream_client: super::streaming_http_client(),
            api_base: api_base.into().trim_end_matches('/').to_owned(),
            login: GrokLogin::new(),
        }
    }

    /// Test constructor that overrides the chat-proxy and OAuth endpoints so
    /// tests never touch the real xAI service.
    pub fn with_endpoints(
        api_base: impl Into<String>,
        device_auth_url: impl Into<String>,
        token_url: impl Into<String>,
    ) -> Self {
        let device_auth_url = device_auth_url.into();
        let token_url = token_url.into();
        let login = if device_auth_url.is_empty() {
            GrokLogin::new()
        } else {
            GrokLogin::with_endpoints(device_auth_url, token_url)
        };
        Self {
            client: reqwest::Client::builder()
                .timeout(GROK_HTTP_TIMEOUT)
                .build()
                .expect("grok provider http client"),
            stream_client: super::streaming_http_client(),
            api_base: api_base.into().trim_end_matches('/').to_owned(),
            login,
        }
    }

    fn access_token(payload: &ProviderPayload) -> Result<String, ProviderError> {
        if payload.as_value().get("provider").and_then(Value::as_str) != Some("grok") {
            return Err(ProviderError::InvalidPayload);
        }
        payload
            .as_value()
            .get("access_token")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .ok_or(ProviderError::InvalidPayload)
    }

    /// 清理 Grok Responses 不接受的工具声明与工具字段。
    ///
    /// cli-chat-proxy 对 `tools[].type` 有固定白名单（`function` / `web_search` /
    /// `x_search` / …，见 [`GROK_TOOL_TYPES`]），白名单之外的声明会让整个请求
    /// 以 422 `unknown variant` 失败；`web_search` 工具内还有若干上游不认的参数
    /// （见 [`GROK_UNSUPPORTED_WEB_SEARCH_FIELDS`]），会以 400
    /// `Argument not supported` 失败。这里只做删除，不猜测转换成其他工具类型，
    /// 也不影响其他 provider。
    ///
    /// 下游客户端（Codex CLI 的 `namespace` / `custom` / `web_search`、
    /// Anthropic 内置工具等）因此不会把整段请求打挂，代价是这些工具对模型不可见。
    fn normalize_responses_body(body: &Value) -> Value {
        let Some(object) = body.as_object() else {
            return body.clone();
        };
        let mut normalized = object.clone();
        let mut removed_tools: Vec<String> = Vec::new();
        let mut stripped_fields = 0usize;

        // include：不再向上游请求 encrypted reasoning blob（拿到了也不可回放）。
        if let Some(include) = normalized.get_mut("include").and_then(Value::as_array_mut) {
            let before = include.len();
            include.retain(|value| value.as_str() != Some(GROK_ENCRYPTED_REASONING_INCLUDE));
            stripped_fields += before.saturating_sub(include.len());
        }
        if normalized
            .get("include")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
        {
            normalized.remove("include");
        }

        // input：历史 reasoning 条目去掉加密 blob；上游要求 content 为数组。
        if let Some(items) = normalized.get_mut("input").and_then(Value::as_array_mut) {
            for item in items.iter_mut() {
                if item.get("type").and_then(Value::as_str) != Some("reasoning") {
                    continue;
                }
                let Some(object) = item.as_object_mut() else {
                    continue;
                };
                if object.remove("encrypted_content").is_some() {
                    stripped_fields += 1;
                }
                if object.get("content").is_some_and(Value::is_null) {
                    object.insert("content".to_owned(), Value::Array(Vec::new()));
                }
            }
        }

        if let Some(tools) = object.get("tools").and_then(Value::as_array) {
            let mut kept = Vec::with_capacity(tools.len());
            for tool in tools {
                if !tool_type_is_supported(tool) {
                    removed_tools.push(tool_marker(tool));
                    continue;
                }
                let (tool, stripped) = strip_unsupported_web_search_fields(tool);
                stripped_fields += stripped;
                kept.push(tool);
            }
            if kept.is_empty() {
                normalized.remove("tools");
            } else {
                normalized.insert("tools".to_owned(), Value::Array(kept));
            }
        }

        // tool_choice 只在有实际工具、且指向仍然存在的工具时保留：上游对
        // “有 tool_choice 但没有 tools”与悬空引用都直接 400。
        let has_tools = normalized
            .get("tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| !tools.is_empty());
        let kept_tool_names: Vec<String> = normalized
            .get("tools")
            .and_then(Value::as_array)
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(|tool| tool.get("name").and_then(Value::as_str))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let removed_choice = match normalized.get("tool_choice").cloned() {
            Some(choice) if !has_tools || !tool_choice_is_supported(&choice, &kept_tool_names) => {
                normalized.remove("tool_choice");
                true
            }
            _ => false,
        };

        // 用内容比较而不是计数器作为“是否真的改过”的判据：`content: null → []`
        // 与空 `include` 移除都不计入 stripped_fields，只看计数器会把
        // “唯一变化就是它们”的请求原样送出去，上游仍会 422。
        if normalized == *object {
            return body.clone();
        }

        tracing::debug!(
            removed_tools = removed_tools.len(),
            removed_tool_choice = removed_choice,
            stripped_fields = stripped_fields,
            "normalized Grok Responses request to the upstream allow-list"
        );
        Value::Object(normalized)
    }

    fn identity_headers(access_token: &str, is_stream: bool) -> Result<HeaderMap, ProviderError> {
        let mut headers = HeaderMap::new();
        let bearer = format!("Bearer {access_token}");
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&bearer).map_err(|_| ProviderError::InvalidPayload)?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            ACCEPT,
            HeaderValue::from_static(if is_stream {
                "text/event-stream"
            } else {
                "application/json"
            }),
        );
        headers.insert(USER_AGENT, HeaderValue::from_static(GROK_USER_AGENT));
        headers.insert(
            HeaderName::from_static(GROK_TOKEN_AUTH_HEADER),
            HeaderValue::from_static(GROK_TOKEN_AUTH_VALUE),
        );
        headers.insert(
            HeaderName::from_static(GROK_CLIENT_VERSION_HEADER),
            HeaderValue::from_static(GROK_CLIENT_VERSION),
        );
        headers.insert(
            HeaderName::from_static(GROK_CLIENT_IDENTIFIER_HEADER),
            HeaderValue::from_static(GROK_CLIENT_IDENTIFIER),
        );
        headers.insert(
            HeaderName::from_static(GROK_AUTHENTICATE_RESPONSE_HEADER),
            HeaderValue::from_static(GROK_AUTHENTICATE_RESPONSE_VALUE),
        );
        Ok(headers)
    }

    fn merge_safe_headers(base: &mut HeaderMap, caller: &HeaderMap) {
        for name in safe_headers() {
            if let Some(value) = caller.get(&name) {
                base.insert(name, value.clone());
            }
        }
    }
}

#[async_trait]
impl Provider for GrokProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Grok
    }

    async fn login(
        &self,
        context: &ProviderLoginContext,
        runtime: &dyn LoginRuntime,
    ) -> Result<LoginResult, ProviderError> {
        match context.login_method {
            super::AuthLoginMode::DeviceCode => {}
            super::AuthLoginMode::BrowserCallback => {
                return Err(ProviderError::UnsupportedFeatures {
                    pointer: "provider.login.grok.browser_callback".into(),
                });
            }
        }
        let result = self.login.login(runtime).await?;
        if let Some(replacement) = &context.replacement {
            // Replacement must keep the same provider identity.  Overwriting
            // account_id would let a different xAI subject clobber this row.
            if result.account_id != replacement.provider_account_id {
                return Err(ProviderError::InvalidPayload);
            }
        }
        Ok(result)
    }

    async fn import(&self, _: &[u8]) -> Result<LoginResult, ProviderError> {
        Err(ProviderError::UnsupportedFeatures {
            pointer: "provider.import.grok".into(),
        })
    }

    async fn refresh(&self, payload: &ProviderPayload) -> Result<RefreshedPayload, ProviderError> {
        self.login.refresh_payload(payload).await
    }

    async fn outbound(
        &self,
        request: ProviderRequest<'_>,
    ) -> Result<reqwest::Response, ProviderError> {
        if request.upstream_protocol != "responses" || request.upstream_endpoint != "responses" {
            return Err(ProviderError::Protocol);
        }
        let access_token = Self::access_token(request.payload)?;
        let mut headers = Self::identity_headers(&access_token, request.is_stream)?;
        Self::merge_safe_headers(&mut headers, request.headers);
        let body = Self::normalize_responses_body(request.body);
        let client = if request.is_stream {
            &self.stream_client
        } else {
            &self.client
        };
        client
            .post(format!("{}/{RESPONSES_PATH}", self.api_base))
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|_| ProviderError::Retryable)
    }

    async fn list_models(
        &self,
        _account: &AuthAccount,
        payload: &ProviderPayload,
    ) -> Result<ProviderModels, ProviderError> {
        let access_token = Self::access_token(payload)?;
        let mut headers = Self::identity_headers(&access_token, false)?;
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let response = self
            .client
            .get(format!("{}/{MODELS_PATH}", self.api_base))
            .headers(headers)
            .send()
            .await
            .map_err(|_| ProviderError::Retryable)?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ProviderError::Unauthorized);
        }
        if !response.status().is_success() {
            return Err(ProviderError::Retryable);
        }
        let body: Value = response.json().await.map_err(|_| ProviderError::Protocol)?;
        normalize_grok_models(&body).ok_or(ProviderError::Protocol)
    }

    async fn fetch_quota(
        &self,
        _account: &AuthAccount,
        _payload: &ProviderPayload,
    ) -> Result<Option<QuotaState>, ProviderError> {
        Ok(None)
    }
}

/// Grok 上游只接受白名单内的 `tools[].type`；未知类型会以 422 让整段请求失败。
fn tool_type_is_supported(tool: &Value) -> bool {
    tool.get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| GROK_TOOL_TYPES.contains(&kind))
}

/// 被移除工具的日志标记（优先名字，其次类型）。
fn tool_marker(tool: &Value) -> String {
    tool.get("name")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| tool.get("type").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| "<unnamed>".to_owned())
}

/// 删除 `web_search` 工具上游不认的字段，返回（工具, 删除字段数）。
fn strip_unsupported_web_search_fields(tool: &Value) -> (Value, usize) {
    if tool.get("type").and_then(Value::as_str) != Some("web_search") {
        return (tool.clone(), 0);
    }
    let Some(object) = tool.as_object() else {
        return (tool.clone(), 0);
    };
    let mut stripped = object.clone();
    let removed = GROK_UNSUPPORTED_WEB_SEARCH_FIELDS
        .iter()
        .filter(|field| stripped.remove(**field).is_some())
        .count();
    (Value::Object(stripped), removed)
}

/// `tool_choice` 是否仍可发送。
///
/// 字符串形态只认 Responses 的三个合法值；对象形态要求类型在工具白名单内，
/// 且 `function` 指向的工具在归一化后仍然存在（悬空引用会被上游 400）。
fn tool_choice_is_supported(choice: &Value, kept_tool_names: &[String]) -> bool {
    if let Some(kind) = choice.as_str() {
        return matches!(kind, "auto" | "none" | "required");
    }
    let Some(object) = choice.as_object() else {
        return false;
    };
    let Some(kind) = object.get("type").and_then(Value::as_str) else {
        return false;
    };
    if !GROK_TOOL_TYPES.contains(&kind) {
        return false;
    }
    if kind != "function" {
        return true;
    }
    let Some(name) = object.get("name").and_then(Value::as_str) else {
        return false;
    };
    kept_tool_names.iter().any(|kept| kept == name)
}

/// Normalize the `/models` snapshot.  Missing or empty ids are skipped; a
/// body that is not an OpenAI-style list fails closed.
fn normalize_grok_models(body: &Value) -> Option<ProviderModels> {
    let entries = body
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| body.get("models").and_then(Value::as_array))
        .or_else(|| body.as_array())?;
    let mut models = Vec::new();
    for entry in entries {
        let id = match entry {
            Value::String(id) => id.as_str(),
            Value::Object(_) => entry.get("id").and_then(Value::as_str).unwrap_or(""),
            _ => continue,
        };
        if id.trim().is_empty() {
            continue;
        }
        models.push(ModelState {
            id: id.to_owned(),
            status: "available".to_owned(),
            unavailable: false,
            next_retry_after: None,
            last_error: None,
            protocol: Some("responses".to_owned()),
        });
    }
    Some(models)
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };

    use axum::{
        extract::State,
        response::IntoResponse,
        routing::{get, post},
        Json, Router,
    };
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use reqwest::header::{HeaderMap, HeaderValue};
    use serde_json::{json, Value};

    use super::super::{LoginRuntime, LoginStep, Provider, ProviderLoginContext};
    use super::*;

    const ACCESS: &str = "fixture-access-token";

    fn fake_jwt(email: &str, subject: &str) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD
            .encode(format!(r#"{{"email":"{email}","sub":"{subject}"}}"#).as_bytes());
        format!("{header}.{payload}.sig")
    }

    #[derive(Clone, Default)]
    struct MockState {
        responses_hits: Arc<AtomicUsize>,
        responses_headers: Arc<Mutex<Vec<HeaderMap>>>,
        bodies: Arc<Mutex<Vec<Value>>>,
        uris: Arc<Mutex<Vec<axum::http::Uri>>>,
        models_status: Arc<AtomicUsize>,
        models_response: Arc<Mutex<Value>>,
        models_headers: Arc<Mutex<Vec<HeaderMap>>>,
    }

    fn account() -> AuthAccount {
        crate::db::models::AuthAccount {
            id: "local-1".into(),
            provider: "grok".into(),
            label: "Grok".into(),
            account_id: "sub-fixture".into(),
            status: "active".into(),
            disabled: 0,
            priority: 0,
            weight: 1,
            sort_order: 0,
            quota_json: None,
            model_states_json: "{}".into(),
            model_mapping_json: "{}".into(),
            model_mapping_disabled: "[]".into(),
            attributes_json: "{}".into(),
            payload_json: json!({
                "provider": "grok",
                "access_token": ACCESS,
                "refresh_token": "fixture-refresh-token"
            })
            .to_string(),
            last_refreshed_at: None,
            last_models_sync_at: None,
            next_refresh_after: None,
            next_retry_after: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn payload() -> ProviderPayload {
        ProviderPayload::new(json!({
            "provider": "grok",
            "access_token": ACCESS,
            "refresh_token": "fixture-refresh-token",
        }))
    }

    fn req<'a>(
        account: &'a AuthAccount,
        payload: &'a ProviderPayload,
        body: &'a Value,
        protocol: &'a str,
        endpoint: &'a str,
        is_stream: bool,
        caller: &'a HeaderMap,
    ) -> ProviderRequest<'a> {
        ProviderRequest {
            account,
            payload,
            body,
            headers: caller,
            is_stream,
            upstream_protocol: protocol,
            upstream_endpoint: endpoint,
        }
    }

    async fn mock_provider() -> (GrokProvider, MockState) {
        let state = MockState::default();
        *state.models_response.lock().unwrap() = json!({
            "data": [{"id": "grok-4"}]
        });
        let app = Router::new()
            .route(
                "/v1/responses",
                post(
                    move |State(s): State<MockState>,
                          uri: axum::extract::OriginalUri,
                          h: HeaderMap,
                          body: axum::body::Bytes| {
                        let s = s.clone();
                        async move {
                            s.responses_hits.fetch_add(1, Ordering::SeqCst);
                            s.uris.lock().unwrap().push(uri.0.clone());
                            s.responses_headers.lock().unwrap().push(h.clone());
                            s.bodies
                                .lock()
                                .unwrap()
                                .push(serde_json::from_slice(&body).unwrap_or(Value::Null));
                            (axum::http::StatusCode::OK, Json(json!({"ok": true})))
                        }
                    },
                ),
            )
            .route(
                "/v1/models",
                get(move |State(s): State<MockState>, h: HeaderMap| async move {
                    s.models_headers.lock().unwrap().push(h.clone());
                    s.uris
                        .lock()
                        .unwrap()
                        .push(axum::http::Uri::from_static("/v1/models"));
                    let status = s.models_status.load(Ordering::SeqCst);
                    if status != 0 {
                        return (
                            axum::http::StatusCode::from_u16(status as u16).unwrap(),
                            Json(json!({"error": "upstream"})),
                        )
                            .into_response();
                    }
                    let body = s.models_response.lock().unwrap().clone();
                    (axum::http::StatusCode::OK, Json(body)).into_response()
                }),
            )
            .route(
                "/oauth/device",
                post(move |_: axum::extract::State<MockState>| async {
                    Json(json!({
                        "device_code": "device-code-1",
                        "user_code": "WXYZ-1234",
                        "verification_uri_complete": "https://accounts.x.ai/sign-in",
                        "expires_in": 1800,
                        "interval": 1
                    }))
                }),
            )
            .route(
                "/oauth/token",
                post(
                    move |_: axum::extract::State<MockState>, body: axum::body::Bytes| async move {
                        let _ = body;
                        Json(json!({
                            "access_token": ACCESS,
                            "refresh_token": "fixture-refresh-token",
                            "token_type": "Bearer",
                            "expires_in": 3600,
                            "id_token": fake_jwt("user@example.test", "sub-fixture")
                        }))
                    },
                ),
            )
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let provider = GrokProvider::with_endpoints(
            format!("http://{addr}/v1"),
            format!("http://{addr}/oauth/device"),
            format!("http://{addr}/oauth/token"),
        );
        (provider, state)
    }

    #[tokio::test]
    async fn kind_is_grok() {
        assert_eq!(GrokProvider::new().kind(), ProviderKind::Grok);
        assert_eq!(GrokProvider::new().api_base, GROK_API_BASE);
        assert!(crate::auth_provider::ProviderRegistry::new()
            .get(&ProviderKind::Grok)
            .is_ok());
    }

    #[test]
    fn normalize_responses_body_removes_namespace_marker_only() {
        let body = json!({
            "model": "grok-4",
            "tools": [
                {
                    "type": "function",
                    "name": "read_file",
                    "parameters": {"type": "object"}
                },
                {"type": "namespace", "name": "multi_agent_v1", "namespace": "multi_agent_v1"},
                {"type": "mcp", "server_label": "github"}
            ],
            "tool_choice": {"type": "namespace", "name": "multi_agent_v1"}
        });

        let normalized = GrokProvider::normalize_responses_body(&body);
        assert_eq!(normalized["tools"].as_array().unwrap().len(), 2);
        assert_eq!(normalized["tools"][0]["type"], "function");
        assert_eq!(normalized["tools"][1]["type"], "mcp");
        assert!(normalized.get("tool_choice").is_none());
    }

    #[test]
    fn normalize_responses_body_removes_empty_namespace_tool_set() {
        let body = json!({
            "model": "grok-4",
            "tools": [{"type": "namespace", "name": "multi_agent_v1"}],
            "tool_choice": {"type": "namespace", "name": "multi_agent_v1"}
        });

        let normalized = GrokProvider::normalize_responses_body(&body);
        assert!(normalized.get("tools").is_none());
        assert!(normalized.get("tool_choice").is_none());
    }

    #[test]
    fn normalize_responses_body_keeps_supported_body_unchanged() {
        let body = json!({
            "model": "grok-4",
            "tools": [{
                "type": "function",
                "name": "read_file",
                "parameters": {"type": "object"}
            }],
            "tool_choice": "auto"
        });

        assert_eq!(GrokProvider::normalize_responses_body(&body), body);
    }

    /// 慢上游回归：`Client::timeout` 是覆盖响应体读取的总超时，用它跑 SSE 会在固定
    /// 秒数处切断长流（下游表现为 `stream interrupted ... operation timed out`）。
    /// 流式出站必须走无总超时的客户端。
    #[tokio::test]
    async fn streaming_outbound_ignores_blocking_total_timeout() {
        let app = Router::new().route(
            "/v1/responses",
            post(|_: axum::body::Bytes| async {
                tokio::time::sleep(std::time::Duration::from_millis(600)).await;
                (
                    axum::http::StatusCode::OK,
                    [("content-type", "text/event-stream")],
                    "data: {\"type\":\"response.completed\"}\n\n",
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let provider = GrokProvider::with_api_base_and_blocking_timeout(
            format!("http://{addr}/v1"),
            std::time::Duration::from_millis(200),
        );
        let caller = HeaderMap::new();

        assert!(
            provider
                .outbound(req(
                    &account(),
                    &payload(),
                    &json!({}),
                    "responses",
                    "responses",
                    true,
                    &caller
                ))
                .await
                .is_ok(),
            "流式请求不应被 200ms 的非流式总超时切断"
        );
        assert!(
            provider
                .outbound(req(
                    &account(),
                    &payload(),
                    &json!({}),
                    "responses",
                    "responses",
                    false,
                    &caller
                ))
                .await
                .is_err(),
            "非流式请求应受总超时约束"
        );
    }

    #[test]
    fn normalize_responses_body_drops_encrypted_reasoning_blobs() {
        // Codex 多轮回放 encrypted_content 时上游 400；同时上游要求 content 是数组。
        let body = json!({
            "model": "grok-4",
            "include": ["reasoning.encrypted_content"],
            "input": [
                {
                    "type": "reasoning",
                    "id": "rs_1",
                    "summary": [{"type": "summary_text", "text": "think"}],
                    "content": null,
                    "encrypted_content": "ZmFrZS1ibG9i"
                },
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]}
            ]
        });

        let normalized = GrokProvider::normalize_responses_body(&body);
        assert!(normalized.get("include").is_none());
        let reasoning = &normalized["input"][0];
        assert!(reasoning.get("encrypted_content").is_none());
        assert_eq!(reasoning["content"], json!([]));
        assert_eq!(reasoning["summary"][0]["text"], "think");
        assert_eq!(normalized["input"][1]["type"], "message");
    }

    #[test]
    fn normalize_responses_body_keeps_other_include_entries() {
        let body = json!({
            "model": "grok-4",
            "include": ["reasoning.encrypted_content", "message.output_text.logprobs"],
            "input": [{"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]}]
        });

        let normalized = GrokProvider::normalize_responses_body(&body);
        assert_eq!(
            normalized["include"],
            json!(["message.output_text.logprobs"])
        );
    }

    #[tokio::test]
    async fn responses_profile_strips_encrypted_reasoning_before_http() {
        let (provider, state) = mock_provider().await;
        let body = json!({
            "model": "grok-4",
            "include": ["reasoning.encrypted_content"],
            "input": [
                {
                    "type": "reasoning",
                    "id": "rs_1",
                    "summary": [],
                    "content": null,
                    "encrypted_content": "ZmFrZS1ibG9i"
                },
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]}
            ]
        });

        provider
            .outbound(req(
                &account(),
                &payload(),
                &body,
                "responses",
                "responses",
                false,
                &HeaderMap::new(),
            ))
            .await
            .unwrap();

        let bodies = state.bodies.lock().unwrap();
        let sent = &bodies[0];
        assert!(sent.get("include").is_none());
        assert!(sent["input"][0].get("encrypted_content").is_none());
        assert_eq!(sent["input"][0]["content"], json!([]));
    }

    #[test]
    fn normalize_responses_body_strips_unsupported_web_search_fields() {
        let body = json!({
            "model": "grok-4",
            "tools": [{
                "type": "web_search",
                "external_web_access": false,
                "search_context_size": "medium",
                "filters": {"allowed_domains": ["example.com"]},
                "user_location": {"type": "approximate", "country": "US"}
            }],
            "tool_choice": "auto"
        });

        let normalized = GrokProvider::normalize_responses_body(&body);
        let tool = &normalized["tools"][0];
        assert!(tool.get("external_web_access").is_none());
        assert!(tool.get("search_context_size").is_none());
        assert_eq!(tool["filters"]["allowed_domains"][0], "example.com");
        assert_eq!(tool["user_location"]["country"], "US");
        assert_eq!(normalized["tool_choice"], "auto");
    }

    #[test]
    fn normalize_responses_body_drops_upstream_unknown_tool_types() {
        // dry-run：Codex 的 custom/local_shell 与 OpenAI 的 web_search_preview 都
        // 不在 cli-chat-proxy 白名单内，必须移除；shell/tool_search 保留。
        let body = json!({
            "model": "grok-4",
            "tools": [
                {"type": "custom", "name": "apply_patch", "format": {"type": "grammar"}},
                {"type": "local_shell"},
                {"type": "web_search_preview"},
                {"type": "shell"},
                {"type": "tool_search"}
            ]
        });

        let normalized = GrokProvider::normalize_responses_body(&body);
        let tools = normalized["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["type"], "shell");
        assert_eq!(tools[1]["type"], "tool_search");
    }

    #[test]
    fn normalize_responses_body_removes_dangling_tool_choice() {
        // 上游对“有 tool_choice 但没有 tools”返回 400，必须一起清理。
        let body = json!({"model": "grok-4", "tool_choice": "auto"});
        let normalized = GrokProvider::normalize_responses_body(&body);
        assert!(normalized.get("tool_choice").is_none());

        // function tool_choice 不能指向已被移除的工具。
        let body = json!({
            "model": "grok-4",
            "tools": [{"type": "function", "name": "read_file", "parameters": {"type": "object"}}],
            "tool_choice": {"type": "function", "name": "apply_patch"}
        });
        let normalized = GrokProvider::normalize_responses_body(&body);
        assert!(normalized.get("tool_choice").is_none());

        // 仍存在的 function 工具引用保持原样。
        let body = json!({
            "model": "grok-4",
            "tools": [{"type": "function", "name": "read_file", "parameters": {"type": "object"}}],
            "tool_choice": {"type": "function", "name": "read_file"}
        });
        let normalized = GrokProvider::normalize_responses_body(&body);
        assert_eq!(normalized["tool_choice"]["name"], "read_file");
    }

    #[tokio::test]
    async fn responses_profile_normalizes_codex_cli_toolset_before_http() {
        let (provider, state) = mock_provider().await;
        let body = json!({
            "model": "grok-4",
            "tools": [
                {"type": "function", "name": "exec_command", "parameters": {"type": "object"}},
                {
                    "type": "namespace",
                    "name": "multi_agent_v1",
                    "tools": [{"type": "function", "name": "close_agent"}]
                },
                {"type": "web_search", "external_web_access": false}
            ],
            "tool_choice": "auto"
        });

        provider
            .outbound(req(
                &account(),
                &payload(),
                &body,
                "responses",
                "responses",
                false,
                &HeaderMap::new(),
            ))
            .await
            .unwrap();

        let bodies = state.bodies.lock().unwrap();
        let sent = &bodies[0];
        let tools = sent["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["name"], "exec_command");
        assert_eq!(tools[1]["type"], "web_search");
        assert!(tools[1].get("external_web_access").is_none());
        assert_eq!(sent["tool_choice"], "auto");
    }

    #[tokio::test]
    async fn responses_profile_filters_namespace_tools_before_http() {
        let (provider, state) = mock_provider().await;
        let body = json!({
            "model": "grok-4",
            "tools": [
                {
                    "type": "function",
                    "name": "read_file",
                    "parameters": {"type": "object"}
                },
                {"type": "namespace", "name": "multi_agent_v1", "namespace": "multi_agent_v1"}
            ],
            "tool_choice": {"type": "namespace", "name": "multi_agent_v1"}
        });

        provider
            .outbound(req(
                &account(),
                &payload(),
                &body,
                "responses",
                "responses",
                false,
                &HeaderMap::new(),
            ))
            .await
            .unwrap();

        let bodies = state.bodies.lock().unwrap();
        let sent = &bodies[0];
        let tools = sent["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert!(sent.get("tool_choice").is_none());
    }

    #[tokio::test]
    async fn responses_profile_uses_bearer_and_blocks_caller_auth() {
        let (provider, state) = mock_provider().await;
        let account = account();
        let mut caller = HeaderMap::new();
        caller.insert(
            reqwest::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer caller-secret"),
        );
        caller.insert(
            reqwest::header::HeaderName::from_static("x-xai-token-auth"),
            HeaderValue::from_static("evil"),
        );
        let body = json!({"model":"grok-4","input":[]});
        provider
            .outbound(req(
                &account,
                &payload(),
                &body,
                "responses",
                "responses",
                true,
                &caller,
            ))
            .await
            .unwrap();
        let headers = state.responses_headers.lock().unwrap();
        let h = &headers[0];
        assert_eq!(
            h.get(reqwest::header::AUTHORIZATION)
                .unwrap()
                .to_str()
                .unwrap(),
            format!("Bearer {ACCESS}")
        );
        assert_eq!(
            h.get("x-xai-token-auth").unwrap().to_str().unwrap(),
            GROK_TOKEN_AUTH_VALUE
        );
        assert_eq!(
            h.get("x-grok-client-identifier").unwrap().to_str().unwrap(),
            GROK_CLIENT_IDENTIFIER
        );
        assert_eq!(
            h.get(reqwest::header::ACCEPT).unwrap().to_str().unwrap(),
            "text/event-stream"
        );
        assert_eq!(state.responses_hits.load(Ordering::SeqCst), 1);
        let uri = &state.uris.lock().unwrap()[0];
        assert_eq!(uri.path(), "/v1/responses");
    }

    #[tokio::test]
    async fn unknown_or_mismatched_profile_fails_before_http() {
        let (provider, state) = mock_provider().await;
        let account = account();
        let body = json!({});
        let result = provider
            .outbound(req(
                &account,
                &payload(),
                &body,
                "openai",
                "chat_completions",
                false,
                &HeaderMap::new(),
            ))
            .await;
        assert_eq!(result.unwrap_err(), ProviderError::Protocol);
        let result = provider
            .outbound(req(
                &account,
                &payload(),
                &body,
                "responses",
                "chat_completions",
                false,
                &HeaderMap::new(),
            ))
            .await;
        assert_eq!(result.unwrap_err(), ProviderError::Protocol);
        assert_eq!(state.responses_hits.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn foreign_payload_fails_closed_before_http() {
        let (provider, state) = mock_provider().await;
        let account = account();
        let foreign = ProviderPayload::new(json!({
            "access_token": ACCESS,
            "device_id": "abc"
        }));
        let result = provider
            .outbound(req(
                &account,
                &foreign,
                &json!({}),
                "responses",
                "responses",
                false,
                &HeaderMap::new(),
            ))
            .await;
        assert_eq!(result.unwrap_err(), ProviderError::InvalidPayload);
        assert_eq!(state.responses_hits.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn safe_passthrough_headers_are_allowed() {
        let (provider, state) = mock_provider().await;
        let mut caller = HeaderMap::new();
        caller.insert(
            reqwest::header::HeaderName::from_static("traceparent"),
            HeaderValue::from_static("00-abc-def-01"),
        );
        provider
            .outbound(req(
                &account(),
                &payload(),
                &json!({"model":"grok-4"}),
                "responses",
                "responses",
                false,
                &caller,
            ))
            .await
            .unwrap();
        let headers = state.responses_headers.lock().unwrap();
        assert_eq!(
            headers[0].get("traceparent").unwrap().to_str().unwrap(),
            "00-abc-def-01"
        );
    }

    #[tokio::test]
    async fn list_models_fetches_fixed_url_and_normalizes() {
        let (provider, state) = mock_provider().await;
        *state.models_response.lock().unwrap() = json!({
            "data": [
                {"id": "grok-4"},
                {"id": ""},
                {"slug": "ignored-without-id"},
                "grok-3"
            ]
        });
        let models = provider.list_models(&account(), &payload()).await.unwrap();
        let ids: Vec<_> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["grok-4", "grok-3"]);
        assert_eq!(models[0].protocol.as_deref(), Some("responses"));
        let uri = &state.uris.lock().unwrap()[0];
        assert_eq!(uri.path(), "/v1/models");
        let h = &state.models_headers.lock().unwrap()[0];
        assert_eq!(
            h.get(reqwest::header::AUTHORIZATION)
                .unwrap()
                .to_str()
                .unwrap(),
            format!("Bearer {ACCESS}")
        );
        assert_eq!(
            h.get("x-xai-token-auth").unwrap().to_str().unwrap(),
            GROK_TOKEN_AUTH_VALUE
        );
    }

    #[tokio::test]
    async fn list_models_401_maps_to_unauthorized() {
        let (provider, state) = mock_provider().await;
        state.models_status.store(401, Ordering::SeqCst);
        let result = provider.list_models(&account(), &payload()).await;
        assert!(matches!(result, Err(ProviderError::Unauthorized)));
    }

    #[tokio::test]
    async fn list_models_malformed_body_fails_closed() {
        let (provider, state) = mock_provider().await;
        *state.models_response.lock().unwrap() = json!({"unexpected": true});
        assert_eq!(
            provider
                .list_models(&account(), &payload())
                .await
                .unwrap_err(),
            ProviderError::Protocol
        );
    }

    #[tokio::test]
    async fn fetch_quota_is_none() {
        let provider = GrokProvider::new();
        assert_eq!(
            provider.fetch_quota(&account(), &payload()).await.unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn import_is_unsupported() {
        let provider = GrokProvider::new();
        assert!(matches!(
            provider.import(b"{}").await.unwrap_err(),
            ProviderError::UnsupportedFeatures { pointer } if pointer == "provider.import.grok"
        ));
    }

    #[tokio::test]
    async fn browser_callback_login_is_rejected() {
        let (provider, _state) = mock_provider().await;
        let runtime = TestRuntime::default();
        let error = provider
            .login(
                &ProviderLoginContext {
                    login_method: crate::auth_provider::AuthLoginMode::BrowserCallback,
                    replacement: None,
                },
                &runtime,
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ProviderError::UnsupportedFeatures { pointer } if pointer.contains("browser_callback")
        ));
    }

    #[tokio::test]
    async fn replacement_login_keeps_matching_account_id() {
        let (provider, _state) = mock_provider().await;
        let runtime = TestRuntime::default();
        let replaced = provider
            .login(
                &ProviderLoginContext {
                    login_method: crate::auth_provider::AuthLoginMode::DeviceCode,
                    replacement: Some(super::super::ReplacementContext {
                        local_account_id: "local-1".into(),
                        provider_account_id: "sub-fixture".into(),
                        previous_payload: payload(),
                        previous_attributes: json!({}),
                    }),
                },
                &runtime,
            )
            .await
            .unwrap();
        assert_eq!(replaced.account_id, "sub-fixture");
    }

    #[tokio::test]
    async fn replacement_login_fails_closed_on_account_id_mismatch() {
        let (provider, _state) = mock_provider().await;
        let runtime = TestRuntime::default();
        let error = provider
            .login(
                &ProviderLoginContext {
                    login_method: crate::auth_provider::AuthLoginMode::DeviceCode,
                    replacement: Some(super::super::ReplacementContext {
                        local_account_id: "local-1".into(),
                        provider_account_id: "existing-sub".into(),
                        previous_payload: payload(),
                        previous_attributes: json!({}),
                    }),
                },
                &runtime,
            )
            .await
            .unwrap_err();
        assert_eq!(error, ProviderError::InvalidPayload);
    }

    #[test]
    fn models_malformed_or_empty_handling() {
        assert!(normalize_grok_models(&json!({"data": "nope"})).is_none());
        assert!(normalize_grok_models(&json!({})).is_none());
        let models = normalize_grok_models(&json!({"data": []})).unwrap();
        assert!(models.is_empty());
    }

    #[derive(Clone)]
    struct TestRuntime {
        cancel: Arc<tokio::sync::watch::Sender<bool>>,
        _rx: Arc<tokio::sync::watch::Receiver<bool>>,
    }
    impl Default for TestRuntime {
        fn default() -> Self {
            let (tx, rx) = tokio::sync::watch::channel(false);
            Self {
                cancel: Arc::new(tx),
                _rx: Arc::new(rx),
            }
        }
    }
    #[async_trait::async_trait]
    impl LoginRuntime for TestRuntime {
        async fn open_browser(&self, _url: &str) -> Result<(), ProviderError> {
            Ok(())
        }
        async fn set_step(&self, _step: LoginStep) {}
        async fn present_device_authorization(
            &self,
            _url: &str,
            _code: &str,
            _expires_at: Option<String>,
        ) -> Result<(), ProviderError> {
            Ok(())
        }
        fn is_cancelled(&self) -> bool {
            *self.cancel.borrow()
        }
        async fn cancelled(&self) {
            std::future::pending::<()>().await;
        }
    }
}
