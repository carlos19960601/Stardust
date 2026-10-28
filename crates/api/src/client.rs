//! 小宇宙（xyz）上游 API 的直连 HTTP 客户端。
//!
//! 本模块直接请求小宇宙上游 API（`https://api.xiaoyuzhoufm.com`），并在请求头中
//! 上报与官方 App 一致的 iOS 头模板（含 `x-jike-access-token` 鉴权头）。
//!
//! 上游响应并不统一：部分接口返回 `{ data: ... }`，部分直接返回数组/对象，
//! 由 [`unwrap_inner`] 统一兼容。登录/刷新返回的 `x-jike-access-token` 与
//! `x-jike-refresh-token` 位于上游响应头中，通过 [`extract_auth_tokens`] 提取。

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;
use thiserror::Error;

use crate::models::*;

/// 小宇宙上游基础地址（业务接口）。
pub const DEFAULT_BASE_URL: &str = "https://api.xiaoyuzhoufm.com";

/// 小宇宙播客创作者后台地址（鉴权接口走此域名的子域）。
const PODCASTER_BASE: &str = "https://podcaster-api.xiaoyuzhoufm.com";
const REFRESH_PATH: &str = "/app_auth_tokens.refresh";

/// 请求超时时间，对齐 horizon 前端的 15s。
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

/// HTTP 方法。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

/// 通用响应信封（兼容上游部分接口的错误返回形态）。
#[derive(Debug, Clone, Deserialize)]
pub struct ApiResponse {
    pub code: i32,
    /// 注意：`/ping` 等个别处理器返回 `message` 而非 `msg`，一并兼容。
    #[serde(default, alias = "message")]
    pub msg: String,
    #[serde(default)]
    pub data: Option<Value>,
    #[serde(default)]
    pub error: Option<String>,
}

/// API 调用错误。
#[derive(Debug, Error)]
pub enum ApiError {
    /// 网络层错误（连接失败、超时等）。
    #[error("网络请求失败：{0}")]
    Network(String),
    /// HTTP 状态码异常且无法解析出业务错误。
    #[error("HTTP 状态码异常（{status}）：{body}")]
    Http { status: u16, body: String },
    /// 服务返回的业务错误。
    #[error("服务返回错误（code={code}）：{msg}")]
    Api { code: i32, msg: String },
    /// 响应反序列化失败。
    #[error("解析 {target} 失败：{detail}")]
    Parse { target: String, detail: String },
}

/// 从响应头中提取到的鉴权令牌对。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthTokens {
    pub access: Option<String>,
    pub refresh: Option<String>,
}

/// 取出信封 `data` 字段中的实际内容：若存在内层 `data` 字段则优先使用，否则原样返回。
///
/// 小宇宙上游 API 的响应并不一致：部分接口（详情、榜单等）返回 `{ data: ... }`，
/// 部分接口（如播放进度列表）直接返回数组/对象，此函数统一兼容两种形态。
pub fn unwrap_inner(value: Value) -> Value {
    match value {
        Value::Object(ref map) => match map.get("data") {
            Some(inner) => inner.clone(),
            None => value,
        },
        other => other,
    }
}

/// 从响应头列表中提取 `x-jike-access-token` / `x-jike-refresh-token`（大小写不敏感）。
pub fn extract_auth_tokens(headers: &[(String, String)]) -> AuthTokens {
    let access = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("x-jike-access-token"))
        .map(|(_, v)| v.clone());
    let refresh = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("x-jike-refresh-token"))
        .map(|(_, v)| v.clone());
    AuthTokens { access, refresh }
}

/// 小宇宙 App 的头模板：语言偏好。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lang {
    HkCn,
    CnTw,
    CnHk,
    TwCn,
}

impl Lang {
    fn as_str(self) -> &'static str {
        match self {
            Lang::HkCn => "zh-Hant-HK;q=1.0, zh-Hans-CN;q=0.9",
            Lang::CnTw => "zh-Hans-CN;q=1.0, zh-Hant-TW;q=0.9",
            Lang::CnHk => "zh-Hans-CN;q=1.0, zh-Hant-HK;q=0.9",
            Lang::TwCn => "zh-Hant-CN;q=1.0, zh-Hans-HK;q=0.9",
        }
    }
}

/// 头模板中「设备标识」的取值方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeviceIdKind {
    None,
    /// 使用客户端生成的随机设备 ID（对齐 Go `utils.DeviceId`）。
    Generated,
    /// 空字符串（部分接口上报空设备标识）。
    Empty,
    /// 固定设备 ID（如查询他人主页时使用的固定值）。
    Fixed(&'static str),
}

/// iOS 请求头模板参数。
#[derive(Debug, Clone, Copy)]
struct Hdr {
    abtest: Option<&'static str>,
    lang: Option<Lang>,
    online_host: Option<&'static str>,
    content_type: Option<(&'static str, &'static str)>,
    device_id: DeviceIdKind,
    device_properties: bool,
    permissions: &'static str,
    ua_build: &'static str,
}

impl Default for Hdr {
    fn default() -> Self {
        Self {
            abtest: Some("enable"),
            lang: Some(Lang::HkCn),
            online_host: None,
            content_type: Some(("Content-Type", "application/json")),
            device_id: DeviceIdKind::None,
            device_properties: false,
            permissions: "4",
            ua_build: "1576",
        }
    }
}

impl Hdr {
    /// GET 请求默认头：与 POST 相同，但无 Content-Type。
    fn get() -> Self {
        Self {
            content_type: None,
            ..Self::default()
        }
    }
}

/// 将 Unix 秒数（Asia/Shanghai，UTC+8）格式化为 `2006-01-02T15:04:05+08:00`。
fn local_time_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        + 8 * 3600;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}+08:00")
}

/// Howard Hinnant 的「自纪元以来的天数 → 公历日期」算法。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// 小宇宙 API 直连客户端。
///
/// 内部持有可变的 access token，供会话期间自动附加鉴权头；
/// 需要切换用户或刷新 token 时调用 [`set_access_token`](Self::set_access_token)。
pub struct ApiClient {
    base_url: String,
    podcaster_base: String,
    access_token: Option<String>,
    device_id: String,
    agent: ureq::Agent,
}

impl ApiClient {
    /// 创建指向指定上游地址的客户端（不带 token）。
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            podcaster_base: PODCASTER_BASE.to_string(),
            access_token: None,
            device_id: uuid::Uuid::new_v4().to_string().to_uppercase(),
            agent: ureq::AgentBuilder::new().timeout(DEFAULT_TIMEOUT).build(),
        }
    }

    /// 同时指定上游业务地址与创作者后台地址（主要用于测试注入）。
    pub fn with_bases(base_url: impl Into<String>, podcaster_base: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            podcaster_base: podcaster_base.into(),
            access_token: None,
            device_id: uuid::Uuid::new_v4().to_string().to_uppercase(),
            agent: ureq::AgentBuilder::new().timeout(DEFAULT_TIMEOUT).build(),
        }
    }

    /// 创建并附带 access token。
    pub fn with_token(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        let mut client = Self::new(base_url);
        client.access_token = Some(token.into());
        client
    }

    /// 返回当前 base URL。
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// 返回当前 access token。
    pub fn access_token(&self) -> Option<&str> {
        self.access_token.as_deref()
    }

    /// 更新 access token。
    pub fn set_access_token(&mut self, token: impl Into<String>) {
        self.access_token = Some(token.into());
    }

    /// 清空 access token。
    pub fn clear_access_token(&mut self) {
        self.access_token = None;
    }

    /// 拼接完整请求地址。
    fn url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }
}

impl ApiClient {
    /// 根据设备标识配置取实际设备 ID 字符串。
    fn device_id_value(&self, kind: DeviceIdKind) -> Option<String> {
        match kind {
            DeviceIdKind::None => None,
            DeviceIdKind::Generated => Some(self.device_id.clone()),
            DeviceIdKind::Empty => Some(String::new()),
            DeviceIdKind::Fixed(s) => Some(s.to_string()),
        }
    }

    /// 构建与官方 iOS App 一致的标准请求头。
    fn ios_headers(&self, h: Hdr) -> Vec<(String, String)> {
        let token = self.access_token.clone().unwrap_or_default();
        let iso = local_time_rfc3339();
        let ua = if h.ua_build == "1858" {
            "Xiaoyuzhou/2.57.1 (build:1858; iOS 17.4.1)"
        } else {
            "Xiaoyuzhou/2.57.1 (build:1576; iOS 17.4.1)"
        };
        let mut v: Vec<(String, String)> = vec![
            ("Host".into(), "api.xiaoyuzhoufm.com".into()),
            ("User-Agent".into(), ua.into()),
            ("Market".into(), "AppStore".into()),
            ("App-BuildNo".into(), "1576".into()),
            ("OS".into(), "ios".into()),
            ("x-jike-access-token".into(), token),
        ];
        if let Some(did) = self.device_id_value(h.device_id) {
            v.push(("x-jike-device-id".into(), did));
        }
        v.push(("Manufacturer".into(), "Apple".into()));
        v.push(("BundleID".into(), "app.podcast.cosmos".into()));
        v.push(("Connection".into(), "keep-alive".into()));
        if let Some(ab) = h.abtest {
            let val = match ab {
                "enable" => "{\"old_user_discovery_feed\":\"enable\"}",
                other => other,
            };
            v.push(("abtest-info".into(), val.to_string()));
        }
        if let Some(lang) = h.lang {
            v.push(("Accept-Language".into(), lang.as_str().to_string()));
        }
        if let Some(oh) = h.online_host {
            v.push((oh.to_string(), "api.xiaoyuzhoufm.com".to_string()));
        }
        v.push(("Model".into(), "iPhone14,2".into()));
        v.push(("app-permissions".into(), h.permissions.to_string()));
        v.push(("Accept".into(), "*/*".into()));
        if let Some((k, val)) = h.content_type {
            v.push((k.to_string(), val.to_string()));
        }
        v.push(("App-Version".into(), "2.57.1".into()));
        v.push(("WifiConnected".into(), "true".into()));
        v.push(("OS-Version".into(), "17.4.1".into()));
        v.push(("x-custom-xiaoyuzhou-app-dev".into(), String::new()));
        v.push(("Local-Time".into(), iso));
        v.push(("Timezone".into(), "Asia/Shanghai".into()));
        if h.device_properties {
            v.push(("x-jike-device-properties".into(), String::new()));
        }
        v
    }

    /// 极简头（关注/粉丝列表、一级评论等仅带 token 与设备 ID 的接口）。
    fn minimal_headers(&self) -> Vec<(String, String)> {
        vec![
            (
                "x-jike-access-token".into(),
                self.access_token.clone().unwrap_or_default(),
            ),
            ("x-jike-device-id".into(), self.device_id.clone()),
            ("Content-Type".into(), "application/json".into()),
        ]
    }

    /// 播客创作者后台（H5）Web 头模板。
    fn podcaster_headers(&self) -> Vec<(String, String)> {
        vec![
            ("accept".into(), "application/json, text/plain, */*".into()),
            (
                "accept-language".into(),
                "zh-CN,zh;q=0.9,en;q=0.8,en-GB;q=0.7,en-US;q=0.6".into(),
            ),
            ("content-type".into(), "application/json;charset=UTF-8".into()),
            ("origin".into(), "https://podcaster.xiaoyuzhoufm.com".into()),
            ("referer".into(), "https://podcaster.xiaoyuzhoufm.com/".into()),
            (
                "User-Agent".into(),
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/146.0.0.0 Safari/537.36 Edg/146.0.0.0".into(),
            ),
        ]
    }

    /// H5 页面头模板（节目主体信息 / 荣誉墙）。
    fn h5_headers(&self, honor: bool) -> Vec<(String, String)> {
        let ua = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_4_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Xiaoyuzhou/2.57.1 (build:1576; iOS 17.4.1) xyzTheme/defaultLight";
        let origin = if honor {
            "https://h5.xiaoyuzhoufm.com/"
        } else {
            "https://terms.xiaoyuzhoufm.com/"
        };
        let mut v = vec![
            ("Host".into(), "api.xiaoyuzhoufm.com".into()),
            ("User-Agent".into(), ua.to_string()),
            ("Referer".into(), origin.to_string()),
            ("Market".into(), "AppStore".into()),
            ("App-BuildNo".into(), "1576".into()),
            ("OS".into(), "ios".into()),
            ("Origin".into(), origin.to_string()),
            ("Sec-Fetch-Dest".into(), "empty".into()),
            ("Sec-Fetch-Site".into(), "same-site".into()),
            ("Sec-Fetch-Mode".into(), "cors".into()),
            (
                "x-jike-access-token".into(),
                self.access_token.clone().unwrap_or_default(),
            ),
            ("Manufacturer".into(), "Apple".into()),
            ("BundleID".into(), "app.podcast.cosmos".into()),
            ("Connection".into(), "keep-alive".into()),
            ("Accept-Language".into(), "zh-CN,zh-Hans;q=0.9".into()),
            ("Model".into(), "iPhone14,2".into()),
            (
                "Accept".into(),
                "application/json, text/plain, */*".into(),
            ),
            ("App-Version".into(), "2.57.1".into()),
            ("OS-Version".into(), "17.4.1".into()),
        ];
        if honor {
            v.push(("Content-type".into(), "application/json".into()));
        }
        v
    }

    /// 刷新 token 专用头（含 access/refresh 双 token 与表单 Content-Type）。
    fn refresh_headers(&self, at: &str, rt: &str) -> Vec<(String, String)> {
        vec![
            ("Host".into(), "api.xiaoyuzhoufm.com".into()),
            (
                "User-Agent".into(),
                "Xiaoyuzhou/2.57.1 (build:1576; iOS 17.4.1)".into(),
            ),
            ("Market".into(), "AppStore".into()),
            ("App-BuildNo".into(), "1576".into()),
            ("OS".into(), "ios".into()),
            ("x-jike-access-token".into(), at.to_string()),
            ("x-jike-refresh-token".into(), rt.to_string()),
            ("Manufacturer".into(), "Apple".into()),
            ("BundleID".into(), "app.podcast.cosmos".into()),
            ("Connection".into(), "keep-alive".into()),
            (
                "Accept-Language".into(),
                "zh-Hant-HK;q=1.0, zh-Hans-CN;q=0.9".into(),
            ),
            ("Model".into(), "iPhone14,2".into()),
            ("app-permissions".into(), "4".into()),
            ("Accept".into(), "*/*".into()),
            (
                "Content-Type".into(),
                "application/x-www-form-urlencoded; charset=utf-8".into(),
            ),
            ("App-Version".into(), "2.57.1".into()),
            ("WifiConnected".into(), "true".into()),
            ("OS-Version".into(), "17.4.1".into()),
            ("x-custom-xiaoyuzhou-app-dev".into(), String::new()),
        ]
    }

    /// 发送请求，返回（响应 JSON, 响应头中的令牌）。不复用信封解析。
    fn call(
        &self,
        method: Method,
        url: &str,
        headers: &[(String, String)],
        body: Option<&[u8]>,
    ) -> Result<(Value, AuthTokens), ApiError> {
        let mut req = match method {
            Method::Get => self.agent.get(url),
            Method::Post => self.agent.post(url),
        };
        for (k, v) in headers {
            req = req.set(k, v);
        }
        let resp = match body {
            Some(b) => req.send_bytes(b),
            None => req.call(),
        };
        let resp = match resp {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                let text = r.into_string().unwrap_or_default();
                if let Ok(env) = serde_json::from_str::<ApiResponse>(&text) {
                    return Err(ApiError::Api {
                        code: env.code,
                        msg: envelope_msg(&env),
                    });
                }
                return Err(ApiError::Http {
                    status: code,
                    body: text,
                });
            }
            Err(ureq::Error::Transport(t)) => return Err(ApiError::Network(t.to_string())),
        };

        let mut hv: Vec<(String, String)> = Vec::new();
        for name in resp.headers_names() {
            if let Some(value) = resp.header(&name) {
                hv.push((name, value.to_string()));
            }
        }
        let tokens = extract_auth_tokens(&hv);

        let text = resp
            .into_string()
            .map_err(|e| ApiError::Network(e.to_string()))?;
        let value = if text.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).map_err(|e| ApiError::Parse {
                target: "响应 JSON".into(),
                detail: e.to_string(),
            })?
        };
        Ok((value, tokens))
    }

    /// 以 base_url 拼接 path 发起请求，仅返回响应 JSON。
    fn exec(
        &self,
        method: Method,
        path: &str,
        headers: &[(String, String)],
        body: Option<Value>,
    ) -> Result<Value, ApiError> {
        let url = self.url(path);
        let bytes = body.map(|v| serde_json::to_vec(&v).unwrap_or_default());
        let (value, _) = self.call(method, &url, headers, bytes.as_deref())?;
        Ok(value)
    }

    /// 发送请求并解析信封，返回 `data` 字段的原始 JSON。
    ///
    /// 直连上游后不再有本地代理信封，`path` 即为上游路径（以 `/` 开头）。
    pub fn raw(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Value, ApiError> {
        let headers = match method {
            Method::Post => self.ios_headers(Hdr::default()),
            Method::Get => self.ios_headers(Hdr::get()),
        };
        let body = body.map(|b| b.clone());
        self.exec(method, path, &headers, body)
    }

    /// 发送请求并把响应直接反序列化为 `T`。
    pub fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<T, ApiError> {
        let data = self.raw(method, path, body)?;
        serde_json::from_value(data).map_err(|e| ApiError::Parse {
            target: std::any::type_name::<T>().into(),
            detail: e.to_string(),
        })
    }

    /// 发送请求并解包内层 `data` 字段后反序列化为 `T`。
    pub fn request_inner<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<T, ApiError> {
        let data = self.raw(method, path, body)?;
        let inner = unwrap_inner(data);
        serde_json::from_value(inner).map_err(|e| ApiError::Parse {
            target: std::any::type_name::<T>().into(),
            detail: e.to_string(),
        })
    }

    /// 便捷的 POST 请求（直接反序列化 `data`）。
    pub fn post<T: DeserializeOwned>(&self, path: &str, body: Value) -> Result<T, ApiError> {
        self.request(Method::Post, path, Some(&body))
    }

    /// 便捷的 POST 请求（解包内层 `data`）。
    pub fn post_inner<T: DeserializeOwned>(&self, path: &str, body: Value) -> Result<T, ApiError> {
        self.request_inner(Method::Post, path, Some(&body))
    }

    /// 返回原始 JSON 的 POST 请求（用于结构尚未建模的接口）。
    pub fn post_raw(&self, path: &str, body: Value) -> Result<Value, ApiError> {
        self.raw(Method::Post, path, Some(&body))
    }
}

impl ApiClient {
    // -----------------------------------------------------------------------
    // 鉴权
    // -----------------------------------------------------------------------

    /// 发送短信验证码。
    pub fn send_code(&self, req: &SendCodeRequest) -> Result<Value, ApiError> {
        let body = serde_json::json!({
            "mobilePhoneNumber": req.mobile_phone_number,
            "areaCode": req.area_code.clone().unwrap_or_else(|| "+86".to_string()),
        });
        let headers = self.podcaster_headers();
        let url = format!("{}/v1/auth/send-code", self.podcaster_base.trim_end_matches('/'));
        let bytes = serde_json::to_vec(&body).unwrap_or_default();
        let (value, _) = self.call(Method::Post, &url, &headers, Some(&bytes))?;
        Ok(value)
    }

    /// 验证码登录，成功后返回用户信息与令牌（令牌取自上游响应头）。
    pub fn login(&self, req: &LoginRequest) -> Result<LoginResult, ApiError> {
        let body = serde_json::json!({
            "areaCode": req.area_code.clone().unwrap_or_else(|| "+86".to_string()),
            "verifyCode": req.verify_code,
            "mobilePhoneNumber": req.mobile_phone_number,
        });
        let headers = self.podcaster_headers();
        let url = format!("{}/v1/auth/login-with-sms", self.podcaster_base.trim_end_matches('/'));
        let bytes = serde_json::to_vec(&body).unwrap_or_default();
        let (value, tokens) = self.call(Method::Post, &url, &headers, Some(&bytes))?;

        #[derive(Deserialize)]
        struct LoginBody {
            data: Option<LoginData>,
        }
        #[derive(Deserialize)]
        struct LoginData {
            user: Option<User>,
        }
        let user = serde_json::from_value::<LoginBody>(value)
            .ok()
            .and_then(|b| b.data)
            .and_then(|d| d.user);

        Ok(LoginResult {
            access_token: tokens.access,
            refresh_token: tokens.refresh,
            data: user,
        })
    }

    /// 刷新 token（新令牌优先取响应头，其次回落到响应体）。
    pub fn refresh_token(&self, req: &RefreshTokenRequest) -> Result<RefreshTokenData, ApiError> {
        let url = format!(
            "{}{}",
            self.base_url.trim_end_matches('/'),
            REFRESH_PATH
        );
        let headers = self.refresh_headers(&req.x_jike_access_token, &req.x_jike_refresh_token);
        let (value, mut tokens) = self.call(Method::Post, &url, &headers, Some(b"null"))?;

        let body_field = |key: &str| -> Option<String> {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_string)
        };
        if tokens.access.is_none() {
            tokens.access = body_field("x-jike-access-token");
        }
        if tokens.refresh.is_none() {
            tokens.refresh = body_field("x-jike-refresh-token");
        }
        Ok(RefreshTokenData {
            access_token: tokens.access,
            refresh_token: tokens.refresh,
        })
    }

    // -----------------------------------------------------------------------
    // 搜索
    // -----------------------------------------------------------------------

    /// 搜索节目/单集/用户。
    pub fn search(&self, req: &SearchRequest) -> Result<SearchResponse, ApiError> {
        let mut body = serde_json::json!({
            "limit": "20",
            "sourcePageName": "4",
            "type": req.kind,
            "currentPageName": "4",
            "keyword": req.keyword,
        });
        if let Some(lmk) = &req.load_more_key {
            body["loadMoreKey"] = lmk.clone();
        }
        if let Some(pid) = &req.pid {
            if !pid.is_empty() {
                body["pid"] = serde_json::Value::String(pid.clone());
            }
        }
        let headers = self.ios_headers(Hdr::default());
        let value = self.exec(Method::Post, "/v1/search/create", &headers, Some(body))?;
        serde_json::from_value(value).map_err(|e| ApiError::Parse {
            target: "SearchResponse".into(),
            detail: e.to_string(),
        })
    }

    /// 「你可能想搜的内容」。
    pub fn search_preset(&self) -> Result<Value, ApiError> {
        let headers = self.ios_headers(Hdr {
            abtest: None,
            ..Hdr::get()
        });
        self.exec(Method::Get, "/v1/search/get-preset", &headers, None)
    }

    // -----------------------------------------------------------------------
    // 单集
    // -----------------------------------------------------------------------

    /// 查询节目内的单集列表。
    pub fn episode_list(&self, req: &EpisodeListRequest) -> Result<EpisodeListResponse, ApiError> {
        let mut body = serde_json::json!({
            "limit": "20",
            "pid": req.pid,
            "order": req.order,
        });
        if let Some(lmk) = &req.load_more_key {
            body["loadMoreKey"] = lmk.clone();
        }
        let headers = self.ios_headers(Hdr::default());
        let value = self.exec(Method::Post, "/v1/episode/list", &headers, Some(body))?;
        serde_json::from_value(value).map_err(|e| ApiError::Parse {
            target: "EpisodeListResponse".into(),
            detail: e.to_string(),
        })
    }

    /// 节目内「最受欢迎」单集列表。
    pub fn episode_list_by_filter(&self, pid: &str) -> Result<EpisodeListResponse, ApiError> {
        let body = serde_json::json!({ "pid": pid, "label": "POPULAR" });
        let headers = self.ios_headers(Hdr {
            abtest: Some("{}"),
            lang: Some(Lang::CnTw),
            ..Hdr::default()
        });
        let value = self.exec(
            Method::Post,
            "/v1/episode/list-by-filter",
            &headers,
            Some(body),
        )?;
        serde_json::from_value(value).map_err(|e| ApiError::Parse {
            target: "EpisodeListResponse".into(),
            detail: e.to_string(),
        })
    }

    /// 查询单集详情。
    pub fn episode_detail(&self, eid: &str) -> Result<Episode, ApiError> {
        let headers = self.ios_headers(Hdr::get());
        let path = format!("/v1/episode/get?eid={}", urlencode(eid));
        let value = self.exec(Method::Get, &path, &headers, None)?;
        serde_json::from_value(unwrap_inner(value)).map_err(|e| ApiError::Parse {
            target: "Episode".into(),
            detail: e.to_string(),
        })
    }

    /// 正在收听该单集的人数。
    pub fn episode_live_count(&self, eid: &str) -> Result<Value, ApiError> {
        let headers = self.ios_headers(Hdr::get());
        let path = format!("/v1/live-stats/episode/get?eid={}", urlencode(eid));
        self.exec(Method::Get, &path, &headers, None)
    }

    /// 查询单集播放进度。
    pub fn episode_play_progress(&self, eids: &[String]) -> Result<Vec<PlaybackProgressEntry>, ApiError> {
        let body = serde_json::json!({ "eids": eids });
        let headers = self.ios_headers(Hdr {
            abtest: None,
            ..Hdr::default()
        });
        let value = self.exec(Method::Post, "/v1/playback-progress/list", &headers, Some(body))?;
        serde_json::from_value(unwrap_inner(value)).map_err(|e| ApiError::Parse {
            target: "Vec<PlaybackProgressEntry>".into(),
            detail: e.to_string(),
        })
    }

    /// 更新单集播放进度。
    pub fn episode_play_progress_update(
        &self,
        data: &[PlaybackProgressItem],
    ) -> Result<Value, ApiError> {
        let body = serde_json::json!({ "data": data });
        let headers = self.ios_headers(Hdr::default());
        self.exec(
            Method::Post,
            "/v1/playback-progress/update",
            &headers,
            Some(body),
        )
    }

    /// 上报播放状态。
    pub fn live_stats_report(&self, req: &LiveStatsReportRequest) -> Result<Value, ApiError> {
        let body = serde_json::json!({
            "playStats": { "action": "PLAY", "eid": req.eid, "pid": req.pid },
        });
        let headers = self.ios_headers(Hdr {
            abtest: Some("{}"),
            lang: Some(Lang::CnTw),
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/live-stats/report", &headers, Some(body))
    }

    /// 精彩时间点。
    pub fn episode_clap(&self, eid: &str, duration: i64) -> Result<Value, ApiError> {
        let body = serde_json::json!({ "eid": eid, "duration": duration });
        let headers = self.ios_headers(Hdr {
            online_host: Some("X-Online-Host"),
            content_type: Some(("Content-type", "application/json")),
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/clap/list", &headers, Some(body))
    }

    /// 标记精彩时间点。
    pub fn episode_clap_create(
        &self,
        eid: &str,
        timestamp: i64,
        duration: i64,
    ) -> Result<Value, ApiError> {
        let body = serde_json::json!({
            "eid": eid,
            "timestamp": timestamp,
            "duration": duration,
            "currentPageName": 7,
            "sourcePageName": 8,
        });
        let headers = self.ios_headers(Hdr {
            online_host: Some("X-Online-Host"),
            content_type: Some(("Content-type", "application/json")),
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/clap/create", &headers, Some(body))
    }

    /// 根据 uid 查询用户的收听历史记录。
    pub fn played_list(&self, uid: &str) -> Result<Value, ApiError> {
        let body = serde_json::json!({ "uid": uid });
        let headers = self.ios_headers(Hdr {
            online_host: Some("X-Online-Host"),
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/episode-played/list", &headers, Some(body))
    }

    // -----------------------------------------------------------------------
    // 节目
    // -----------------------------------------------------------------------

    /// 查询节目详情。
    pub fn podcast_detail(&self, pid: &str) -> Result<Podcast, ApiError> {
        let headers = self.ios_headers(Hdr::get());
        let path = format!("/v1/podcast/get?pid={}", urlencode(pid));
        let value = self.exec(Method::Get, &path, &headers, None)?;
        serde_json::from_value(unwrap_inner(value)).map_err(|e| ApiError::Parse {
            target: "Podcast".into(),
            detail: e.to_string(),
        })
    }

    /// 获取节目主体信息。
    pub fn podcast_get_info(&self, pid: &str) -> Result<Podcast, ApiError> {
        let headers = self.h5_headers(false);
        let path = format!("/v1/podcast/get-info?pid={}", urlencode(pid));
        let value = self.exec(Method::Get, &path, &headers, None)?;
        serde_json::from_value(unwrap_inner(value)).map_err(|e| ApiError::Parse {
            target: "Podcast".into(),
            detail: e.to_string(),
        })
    }

    /// 获取节目荣誉墙。
    pub fn podcast_honor_list(&self, pid: &str) -> Result<Vec<PodcastHonor>, ApiError> {
        let body = serde_json::json!({ "pid": pid });
        let headers = self.h5_headers(true);
        let value = self.exec(Method::Post, "/v1/podcast-honor/list", &headers, Some(body))?;
        serde_json::from_value(unwrap_inner(value)).map_err(|e| ApiError::Parse {
            target: "Vec<PodcastHonor>".into(),
            detail: e.to_string(),
        })
    }

    /// 相关节目推荐。
    pub fn podcast_related(&self, pid: &str) -> Result<Value, ApiError> {
        let body = serde_json::json!({ "pid": pid, "position": "BOTTOM" });
        let headers = self.ios_headers(Hdr {
            content_type: Some(("Content-type", "application/json")),
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/related-podcast/list", &headers, Some(body))
    }

    /// 获取节目公告。
    pub fn podcast_bulletin(&self, pid: &str) -> Result<PodcastBulletin, ApiError> {
        let headers = self.ios_headers(Hdr {
            abtest: Some("{}"),
            lang: Some(Lang::CnTw),
            ua_build: "1858",
            ..Hdr::get()
        });
        let path = format!("/v1/podcast-bulletin/get-by-pid?pid={}", urlencode(pid));
        let value = self.exec(Method::Get, &path, &headers, None)?;
        serde_json::from_value(unwrap_inner(value)).map_err(|e| ApiError::Parse {
            target: "PodcastBulletin".into(),
            detail: e.to_string(),
        })
    }

    /// 查询用户创建的播客节目。
    pub fn owned_podcasts(&self, uid: &str) -> Result<Vec<Podcast>, ApiError> {
        let body = serde_json::json!({ "uid": uid });
        let headers = self.ios_headers(Hdr {
            content_type: Some(("Content-type", "application/json")),
            ..Hdr::default()
        });
        let value = self.exec(Method::Post, "/v1/podcaster/owned-podcasts", &headers, Some(body))?;
        serde_json::from_value(unwrap_inner(value)).map_err(|e| ApiError::Parse {
            target: "Vec<Podcast>".into(),
            detail: e.to_string(),
        })
    }

    // -----------------------------------------------------------------------
    // 订阅
    // -----------------------------------------------------------------------

    /// 我的订阅列表。
    pub fn subscription(&self, req: &SubscriptionRequest) -> Result<SubscriptionResponse, ApiError> {
        let mut body = serde_json::json!({
            "limit": "20",
            "sortOrder": "desc",
            "sortBy": "subscribedAt",
        });
        if let Some(uid) = &req.uid {
            if !uid.is_empty() {
                body["uid"] = serde_json::Value::String(uid.clone());
            }
        }
        if let Some(lmk) = &req.load_more_key {
            body["loadMoreKey"] = lmk.clone();
        }
        let headers = self.ios_headers(Hdr::default());
        let value = self.exec(Method::Post, "/v1/subscription/list", &headers, Some(body))?;
        serde_json::from_value(value).map_err(|e| ApiError::Parse {
            target: "SubscriptionResponse".into(),
            detail: e.to_string(),
        })
    }

    /// 更新订阅状态。
    pub fn subscription_update(&self, req: &SubscriptionUpdateRequest) -> Result<Value, ApiError> {
        let body = serde_json::json!({ "pid": req.pid, "mode": req.status });
        let headers = self.ios_headers(Hdr {
            abtest: Some("{}"),
            lang: Some(Lang::CnTw),
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/subscription/update", &headers, Some(body))
    }

    /// 星标订阅列表。
    pub fn subscription_star(&self) -> Result<Value, ApiError> {
        let headers = self.ios_headers(Hdr::default());
        self.exec(
            Method::Post,
            "/v1/subscription-star/list",
            &headers,
            Some(serde_json::json!({})),
        )
    }

    /// 未加星标订阅列表。
    pub fn subscription_non_starred(&self) -> Result<Value, ApiError> {
        let headers = self.ios_headers(Hdr::default());
        self.exec(
            Method::Post,
            "/v1/subscription/list-non-starred",
            &headers,
            Some(serde_json::json!({})),
        )
    }

    /// 更新星标订阅。
    pub fn subscription_star_update(&self, pid: &str, with_star: bool) -> Result<Value, ApiError> {
        let body = serde_json::json!({ "pid": pid, "withStar": with_star });
        let headers = self.ios_headers(Hdr::default());
        self.exec(Method::Post, "/v1/subscription-star/update", &headers, Some(body))
    }

    // -----------------------------------------------------------------------
    // 评论
    // -----------------------------------------------------------------------

    /// 查询单集的评论。
    pub fn comment_primary(
        &self,
        owner_id: &str,
        owner_type: &str,
        order: &str,
        load_more_key: Option<CommentPrimaryLoadMoreKey>,
    ) -> Result<CommentListResponse, ApiError> {
        let mut body = serde_json::json!({
            "order": order,
            "owner": { "id": owner_id, "type": owner_type },
        });
        if let Some(key) = load_more_key {
            body["loadMoreKey"] = serde_json::json!({
                "hotSortScore": key.hot_sort_score,
                "id": key.id,
                "direction": key.direction,
                "section": key.section,
            });
        }
        let headers = self.minimal_headers();
        let value = self.exec(Method::Post, "/v1/comment/list-primary", &headers, Some(body))?;
        serde_json::from_value(value).map_err(|e| ApiError::Parse {
            target: "CommentListResponse".into(),
            detail: e.to_string(),
        })
    }

    /// 查询回复评论。
    pub fn comment_thread(
        &self,
        order: &str,
        primary_comment_id: &str,
    ) -> Result<CommentListResponse, ApiError> {
        let body = serde_json::json!({ "order": order, "primaryCommentId": primary_comment_id });
        let headers = self.ios_headers(Hdr {
            online_host: Some("X-Online-Host"),
            device_id: DeviceIdKind::Generated,
            ..Hdr::default()
        });
        let value = self.exec(Method::Post, "/v1/comment/list-thread", &headers, Some(body))?;
        serde_json::from_value(value).map_err(|e| ApiError::Parse {
            target: "CommentListResponse".into(),
            detail: e.to_string(),
        })
    }

    /// 收藏评论。
    pub fn comment_collect_create(&self, comment_id: &str) -> Result<Value, ApiError> {
        let body = serde_json::json!({ "commentId": comment_id });
        let headers = self.ios_headers(Hdr {
            online_host: Some("X-Online-Host"),
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/comment/collect/create", &headers, Some(body))
    }

    /// 取消收藏评论。
    pub fn comment_collect_remove(&self, comment_id: &str) -> Result<Value, ApiError> {
        let body = serde_json::json!({ "commentId": comment_id });
        let headers = self.ios_headers(Hdr {
            online_host: Some("X-Online-Host"),
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/comment/collect/remove", &headers, Some(body))
    }

    /// 获取收藏评论列表。
    pub fn comment_collect_list(&self, load_more_key: Option<&str>) -> Result<Value, ApiError> {
        let mut body = serde_json::json!({});
        if let Some(key) = load_more_key {
            if !key.is_empty() {
                body["loadMoreKey"] = serde_json::Value::String(key.to_string());
            }
        }
        let headers = self.ios_headers(Hdr {
            abtest: None,
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/comment/collect/list", &headers, Some(body))
    }

    /// 点赞/取消点赞评论（`target_type` 仅支持 `COMMENT` 或 `PICK`）。
    pub fn comment_like_update(
        &self,
        liked: bool,
        target_id: &str,
        target_type: &str,
    ) -> Result<Value, ApiError> {
        let body = serde_json::json!({
            "liked": liked,
            "target": { "id": target_id, "type": target_type },
        });
        let headers = self.ios_headers(Hdr {
            online_host: Some("X-Online-Host"),
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/like/update", &headers, Some(body))
    }

    /// 创建评论；传入 `reply_to_comment_id` 时为回复他人评论，否则给单集添加评论。
    pub fn comment_create(
        &self,
        text: &str,
        owner_id: &str,
        owner_type: &str,
        reply_to_comment_id: Option<&str>,
    ) -> Result<Value, ApiError> {
        let mut body = serde_json::json!({
            "text": text,
            "owner": { "id": owner_id, "type": owner_type },
        });
        if let Some(rid) = reply_to_comment_id {
            if !rid.is_empty() {
                body["replyToCommentId"] = serde_json::Value::String(rid.to_string());
            }
        }
        let headers = self.ios_headers(Hdr {
            abtest: Some("{}"),
            lang: Some(Lang::CnHk),
            online_host: Some("X-Online_Host"),
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/comment/create", &headers, Some(body))
    }

    /// 删除评论。
    pub fn comment_remove(&self, comment_id: &str) -> Result<Value, ApiError> {
        let body = serde_json::json!({ "commentId": comment_id });
        let headers = self.ios_headers(Hdr {
            abtest: Some("{}"),
            lang: Some(Lang::CnHk),
            online_host: Some("X-Online-Host"),
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/comment/remove", &headers, Some(body))
    }

    // -----------------------------------------------------------------------
    // 首页发现 / 榜单
    // -----------------------------------------------------------------------

    /// 首页推荐、精选、榜单等模块。
    pub fn discovery(&self, load_more_key: Option<&str>) -> Result<DiscoveryResponse, ApiError> {
        let mut body = serde_json::json!({ "returnAll": "false" });
        if let Some(key) = load_more_key {
            if !key.is_empty() {
                body["loadMoreKey"] = serde_json::Value::String(key.to_string());
            }
        }
        let headers = self.ios_headers(Hdr::default());
        let value = self.exec(Method::Post, "/v1/discovery-feed/list", &headers, Some(body))?;
        serde_json::from_value(value).map_err(|e| ApiError::Parse {
            target: "DiscoveryResponse".into(),
            detail: e.to_string(),
        })
    }

    /// 获取最热榜/锋芒榜/新星榜。
    pub fn top_list(&self, category: &str) -> Result<TopList, ApiError> {
        let mapped = match category {
            "HOT" => "HOT_EPISODES_IN_24_HOURS",
            "ROCK" => "SKYROCKET_EPISODES",
            "NEW" => "NEW_STAR_EPISODES",
            _ => "HOT_EPISODES_IN_24_HOURS",
        };
        let headers = self.ios_headers(Hdr::get());
        let path = format!("/v1/top-list/get?category={}", mapped);
        let value = self.exec(Method::Get, &path, &headers, None)?;
        serde_json::from_value(unwrap_inner(value)).map_err(|e| ApiError::Parse {
            target: "TopList".into(),
            detail: e.to_string(),
        })
    }

    /// 编辑精选-往日精选历史。
    pub fn editor_pick_list_history(&self, load_more_key: Option<&str>) -> Result<Value, ApiError> {
        let mut body = serde_json::json!({});
        if let Some(key) = load_more_key {
            if !key.is_empty() {
                body["loadMoreKey"] = serde_json::Value::String(key.to_string());
            }
        }
        let headers = self.ios_headers(Hdr {
            abtest: Some("{}"),
            lang: Some(Lang::TwCn),
            online_host: Some("X-Online-Host"),
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/editor-pick/list-history", &headers, Some(body))
    }

    /// 新节目广场。
    pub fn pilot_discovery_list(&self) -> Result<Value, ApiError> {
        let headers = self.ios_headers(Hdr {
            abtest: Some("{}"),
            lang: Some(Lang::TwCn),
            ..Hdr::default()
        });
        self.exec(
            Method::Post,
            "/v1/pilot-discovery/list",
            &headers,
            Some(serde_json::json!({})),
        )
    }

    /// 首页「大家都在听」刷新推荐。
    pub fn refresh_episode_recommend(&self) -> Result<Value, ApiError> {
        let headers = self.ios_headers(Hdr::default());
        self.exec(
            Method::Post,
            "/v1/discovery-collection/refresh-episode-recommend",
            &headers,
            Some(serde_json::json!({})),
        )
    }

    // -----------------------------------------------------------------------
    // 用户 / 个人资料
    // -----------------------------------------------------------------------

    /// 根据 uid 查询用户信息（上游 Profile 接口忽略 uid，返回当前登录用户）。
    pub fn profile(&self, _uid: &str) -> Result<BaseUser, ApiError> {
        let headers = self.ios_headers(Hdr::get());
        let value = self.exec(Method::Get, "/v1/profile/get", &headers, None)?;
        serde_json::from_value(unwrap_inner(value)).map_err(|e| ApiError::Parse {
            target: "BaseUser".into(),
            detail: e.to_string(),
        })
    }

    /// 根据 uid 查询用户信息（get_profile 别名）。
    pub fn get_profile(&self, uid: &str) -> Result<BaseUser, ApiError> {
        let headers = self.ios_headers(Hdr {
            device_id: DeviceIdKind::Fixed("81ADBFD6-6921-482B-9AB9-A29E7CC7BB55"),
            ..Hdr::get()
        });
        let path = format!("/v1/profile/get?uid={}", urlencode(uid));
        let value = self.exec(Method::Get, &path, &headers, None)?;
        serde_json::from_value(unwrap_inner(value)).map_err(|e| ApiError::Parse {
            target: "BaseUser".into(),
            detail: e.to_string(),
        })
    }

    /// 用户统计数据。
    pub fn user_stats(&self, uid: &str) -> Result<UserStats, ApiError> {
        let headers = self.ios_headers(Hdr {
            device_id: DeviceIdKind::Fixed("81ADBFD6-6921-482B-9AB9-A29E7CC7BB55"),
            ..Hdr::get()
        });
        let path = format!("/v1/user-stats/get?uid={}", urlencode(uid));
        let value = self.exec(Method::Get, &path, &headers, None)?;
        serde_json::from_value(unwrap_inner(value)).map_err(|e| ApiError::Parse {
            target: "UserStats".into(),
            detail: e.to_string(),
        })
    }

    /// 根据 uid 查询已获得的贴纸。
    pub fn sticker(&self, uid: &str) -> Result<Value, ApiError> {
        let body = serde_json::json!({ "uid": uid });
        let headers = self.ios_headers(Hdr::default());
        self.exec(Method::Post, "/v1/sticker/list", &headers, Some(body))
    }

    /// 查询我的贴纸墙。
    pub fn sticker_board(&self, uid: &str) -> Result<Value, ApiError> {
        let body = serde_json::json!({ "uid": uid });
        let headers = self.ios_headers(Hdr {
            abtest: None,
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/sticker/get-board", &headers, Some(body))
    }

    // -----------------------------------------------------------------------
    // 收藏 / 关注 / 历史 / 设置
    // -----------------------------------------------------------------------

    /// 更新收藏单集。
    pub fn favorite_episode_update(&self, eid: &str, favorited: bool) -> Result<Value, ApiError> {
        let body = serde_json::json!({
            "eid": eid,
            "favorited": favorited,
            "sourcePageName": 8,
            "currentPageName": 9,
        });
        let headers = self.ios_headers(Hdr {
            abtest: None,
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/favorite/update", &headers, Some(body))
    }

    /// 获取收藏单集列表。
    pub fn favorite_episode_list(&self) -> Result<Value, ApiError> {
        let headers = self.ios_headers(Hdr {
            abtest: Some("{}"),
            lang: Some(Lang::CnTw),
            device_id: DeviceIdKind::Empty,
            device_properties: true,
            ..Hdr::default()
        });
        self.exec(
            Method::Post,
            "/v1/favorite/list",
            &headers,
            Some(serde_json::json!({})),
        )
    }

    /// 收听历史。
    pub fn episode_played_history_list(&self, load_more_key: Option<&str>) -> Result<Value, ApiError> {
        let mut body = serde_json::json!({});
        if let Some(key) = load_more_key {
            if !key.is_empty() {
                body["loadMoreKey"] = serde_json::Value::String(key.to_string());
            }
        }
        let headers = self.ios_headers(Hdr {
            abtest: None,
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/episode-played/list-history", &headers, Some(body))
    }

    /// 更新收听历史。
    pub fn episode_played_history_list_update(&self, eid: &str) -> Result<Value, ApiError> {
        let body = serde_json::json!({ "eid": eid });
        let headers = self.ios_headers(Hdr {
            abtest: Some("{}"),
            lang: Some(Lang::CnTw),
            online_host: Some("X-Online-Host"),
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/episode-played/create", &headers, Some(body))
    }

    /// 订阅更新列表。
    pub fn inbox_list(&self, load_more_key: Option<InboxLoadMoreKey>) -> Result<Value, ApiError> {
        let mut body = serde_json::json!({ "limit": "20" });
        if let Some(key) = load_more_key {
            body["loadMoreKey"] = serde_json::json!({ "pubDate": key.pub_date, "id": key.id });
        }
        let headers = self.ios_headers(Hdr {
            abtest: None,
            lang: Some(Lang::CnTw),
            device_id: DeviceIdKind::Empty,
            device_properties: true,
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/inbox/list", &headers, Some(body))
    }

    /// 未读消息数。
    pub fn unread_count(&self) -> Result<Value, ApiError> {
        let headers = self.ios_headers(Hdr {
            online_host: Some("X-Online-Host"),
            ..Hdr::get()
        });
        self.exec(Method::Get, "/v1/unread-count/get", &headers, None)
    }

    /// 获取「我」关注的人。
    pub fn following_list(&self, uid: &str) -> Result<Vec<BaseUser>, ApiError> {
        let body = serde_json::json!({ "uid": uid });
        let headers = self.minimal_headers();
        let value = self.exec(Method::Post, "/v1/user-relation/list-following", &headers, Some(body))?;
        serde_json::from_value(unwrap_inner(value)).map_err(|e| ApiError::Parse {
            target: "Vec<BaseUser>".into(),
            detail: e.to_string(),
        })
    }

    /// 获取关注「我」的人。
    pub fn follower_list(&self, uid: &str) -> Result<Vec<BaseUser>, ApiError> {
        let body = serde_json::json!({ "uid": uid });
        let headers = self.minimal_headers();
        let value = self.exec(Method::Post, "/v1/user-relation/list-follower", &headers, Some(body))?;
        serde_json::from_value(unwrap_inner(value)).map_err(|e| ApiError::Parse {
            target: "Vec<BaseUser>".into(),
            detail: e.to_string(),
        })
    }

    /// 关注/取关用户。
    pub fn relation_update(&self, uid: &str, relation: &str) -> Result<Value, ApiError> {
        let body = serde_json::json!({ "uid": uid, "relation": relation });
        let headers = self.ios_headers(Hdr {
            abtest: Some("{}"),
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/user-relation/update", &headers, Some(body))
    }

    /// 获取用户偏好设置。
    pub fn user_preference_get(&self) -> Result<Value, ApiError> {
        let headers = self.ios_headers(Hdr::get());
        self.exec(Method::Get, "/v1/user-preference/get", &headers, None)
    }

    /// 更新用户偏好设置（动态 key，值必须为 bool）。
    pub fn user_preference_update(&self, key: &str, flag: bool) -> Result<Value, ApiError> {
        let mut map = serde_json::Map::new();
        map.insert(key.to_string(), serde_json::Value::Bool(flag));
        let body = serde_json::Value::Object(map);
        // Go 侧该接口为 POST 但不带 Content-Type。
        let headers = self.ios_headers(Hdr {
            content_type: None,
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/user-preference/update", &headers, Some(body))
    }

    /// 获取付费单集音频链接。
    pub fn private_media_get(&self, eid: &str) -> Result<Value, ApiError> {
        let headers = self.ios_headers(Hdr {
            abtest: None,
            lang: None,
            permissions: "100000",
            ..Hdr::get()
        });
        let path = format!("/v1/private-media/get?dubbing=false&eid={}", urlencode(eid));
        self.exec(Method::Get, &path, &headers, None)
    }

    // -----------------------------------------------------------------------
    // 分类
    // -----------------------------------------------------------------------

    /// 全部分类。
    pub fn category_list(&self) -> Result<Value, ApiError> {
        let headers = self.ios_headers(Hdr {
            abtest: None,
            ..Hdr::default()
        });
        self.exec(
            Method::Post,
            "/v1/category/list-all",
            &headers,
            Some(serde_json::json!({})),
        )
    }

    /// 获取分类下的标签。
    pub fn category_list_tab(&self, category_id: &str) -> Result<Value, ApiError> {
        let body = serde_json::json!({ "categoryId": category_id });
        let headers = self.ios_headers(Hdr {
            abtest: None,
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/category/podcast/list-tabs", &headers, Some(body))
    }

    /// 根据标签获取分类下的节目列表。
    pub fn category_podcast_list(
        &self,
        category_id: &str,
        omit_subscribed: bool,
        tab: &str,
        load_more_key: Option<i64>,
    ) -> Result<Value, ApiError> {
        let mut body = serde_json::json!({
            "categoryId": category_id,
            "omitSubscribed": omit_subscribed,
            "tab": tab,
        });
        if let Some(k) = load_more_key {
            if k != 0 {
                body["loadMoreKey"] = serde_json::Value::from(k);
            }
        }
        let headers = self.ios_headers(Hdr {
            abtest: None,
            ..Hdr::default()
        });
        self.exec(
            Method::Post,
            "/v1/category/podcast/list-by-tab",
            &headers,
            Some(body),
        )
    }

    // -----------------------------------------------------------------------
    // 里程（收听数据）
    // -----------------------------------------------------------------------

    /// 获取收听数据概览。
    pub fn mileage_get(&self) -> Result<Value, ApiError> {
        // Go 侧为 GET 但带 Content-Type。
        let headers = self.ios_headers(Hdr::default());
        self.exec(Method::Get, "/v1/mileage/get", &headers, None)
    }

    /// 获取收听数据排行榜。
    pub fn mileage_list(&self, all: bool) -> Result<Value, ApiError> {
        let rank = if all { "TOTAL" } else { "LAST_THIRTY_DAYS" };
        let body = serde_json::json!({ "rank": rank });
        let headers = self.ios_headers(Hdr {
            abtest: Some("{}"),
            lang: Some(Lang::CnTw),
            online_host: Some("X-Online-Host"),
            device_id: DeviceIdKind::Empty,
            device_properties: true,
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/mileage/list", &headers, Some(body))
    }

    /// 更新收听数据概览。
    pub fn mileage_update(&self, tracking: &[MileageTracking]) -> Result<Value, ApiError> {
        let body = serde_json::json!({ "tracking": tracking });
        let headers = self.ios_headers(Hdr {
            abtest: Some("{}"),
            lang: Some(Lang::CnTw),
            online_host: Some("X-Online-Host"),
            device_id: DeviceIdKind::Empty,
            device_properties: true,
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/mileage/update", &headers, Some(body))
    }

    // -----------------------------------------------------------------------
    // 喜欢（pick）
    // -----------------------------------------------------------------------

    /// 个人主页「用户的喜欢」部分展示片段。
    pub fn pick_list_recent(&self, uid: &str) -> Result<Value, ApiError> {
        let body = serde_json::json!({ "uid": uid });
        let headers = self.ios_headers(Hdr::default());
        self.exec(Method::Post, "/v1/pick/list-recent", &headers, Some(body))
    }

    /// 个人主页「用户的喜欢」全部内容。
    pub fn pick_list_history(&self, uid: &str, load_more_key: Option<&str>) -> Result<Value, ApiError> {
        let mut body = serde_json::json!({ "uid": uid });
        if let Some(k) = load_more_key {
            if !k.is_empty() {
                body["loadMoreKey"] = serde_json::Value::String(k.to_string());
            }
        }
        let headers = self.ios_headers(Hdr::default());
        self.exec(Method::Post, "/v1/pick/list-history", &headers, Some(body))
    }

    // -----------------------------------------------------------------------
    // 黑名单
    // -----------------------------------------------------------------------

    /// 查询黑名单列表。
    pub fn blocked_user_lists(&self) -> Result<Value, ApiError> {
        let headers = self.minimal_headers();
        self.exec(
            Method::Post,
            "/v1/blocked-user/list",
            &headers,
            Some(serde_json::json!({})),
        )
    }

    /// 将用户加入黑名单。
    pub fn blocked_user_create(&self, uid: &str) -> Result<Value, ApiError> {
        let body = serde_json::json!({ "uid": uid });
        let headers = self.ios_headers(Hdr {
            online_host: Some("X-Online-Host"),
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/blocked-user/create", &headers, Some(body))
    }

    /// 将用户移出黑名单。
    pub fn blocked_user_remove(&self, uid: &str) -> Result<Value, ApiError> {
        let body = serde_json::json!({ "uid": uid });
        let headers = self.ios_headers(Hdr {
            online_host: Some("X-Online-Host"),
            ..Hdr::default()
        });
        self.exec(Method::Post, "/v1/blocked-user/remove", &headers, Some(body))
    }
}

/// 从信封中取出可读的错误信息：优先 `msg`，其次 `error`。
fn envelope_msg(env: &ApiResponse) -> String {
    if env.msg.is_empty() {
        env.error.clone().unwrap_or_default()
    } else {
        env.msg.clone()
    }
}

/// 对查询参数做最小 URL 编码（eid/pid 为十六进制 ID，理论上无需转义）。
fn urlencode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ' ' => "%20".to_string(),
            '&' => "%26".to_string(),
            '#' => "%23".to_string(),
            '?' => "%3F".to_string(),
            '%' => "%25".to_string(),
            _ => c.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn 解包内层data_兼容双重与单层() {
        let v = unwrap_inner(json!({ "data": { "eid": "e1" } }));
        assert_eq!(v, json!({ "eid": "e1" }));
        let v = unwrap_inner(json!({ "eid": "e1" }));
        assert_eq!(v, json!({ "eid": "e1" }));
        let v = unwrap_inner(json!([1, 2, 3]));
        assert_eq!(v, json!([1, 2, 3]));
        let v = unwrap_inner(json!({ "data": null }));
        assert_eq!(v, Value::Null);
    }

    #[test]
    fn 解析响应信封() {
        let env: ApiResponse =
            serde_json::from_str(r#"{"code":200,"msg":"OK","data":{"eid":"e1"}}"#).unwrap();
        assert_eq!(env.code, 200);
        assert_eq!(env.msg, "OK");
        assert_eq!(env.data, Some(json!({ "eid": "e1" })));
        assert_eq!(envelope_msg(&env), "OK");
    }

    #[test]
    fn 错误信封_优先msg其次error() {
        let env: ApiResponse =
            serde_json::from_str(r#"{"code":400,"msg":"","error":"bad"}"#).unwrap();
        assert_eq!(envelope_msg(&env), "bad");
    }

    #[test]
    fn 客户端url拼接() {
        let c = ApiClient::new("https://api.xiaoyuzhoufm.com/");
        assert_eq!(c.url("/ping"), "https://api.xiaoyuzhoufm.com/ping");
        assert_eq!(c.url("ping"), "https://api.xiaoyuzhoufm.com/ping");
        let c2 = ApiClient::new("https://api.xiaoyuzhoufm.com");
        assert_eq!(c2.url("/search"), "https://api.xiaoyuzhoufm.com/search");
    }

    #[test]
    fn 默认地址指向小宇宙上游() {
        assert_eq!(DEFAULT_BASE_URL, "https://api.xiaoyuzhoufm.com");
    }

    #[test]
    fn token读写() {
        let mut c = ApiClient::new(DEFAULT_BASE_URL);
        assert_eq!(c.access_token(), None);
        c.set_access_token("tok");
        assert_eq!(c.access_token(), Some("tok"));
        c.clear_access_token();
        assert_eq!(c.access_token(), None);
    }

    #[test]
    fn 从响应头提取token_大小写不敏感() {
        let headers = vec![
            ("X-Jike-Access-Token".to_string(), "AT-1".to_string()),
            ("x-jike-refresh-token".to_string(), "RT-1".to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
        ];
        let tokens = extract_auth_tokens(&headers);
        assert_eq!(tokens.access.as_deref(), Some("AT-1"));
        assert_eq!(tokens.refresh.as_deref(), Some("RT-1"));

        let empty: Vec<(String, String)> = Vec::new();
        let tokens = extract_auth_tokens(&empty);
        assert_eq!(tokens, AuthTokens::default());
    }

    #[test]
    fn ios头模板_包含关键字段() {
        let c = ApiClient::with_token(DEFAULT_BASE_URL, "AT-2");
        let headers = c.ios_headers(Hdr::default());
        let get = |k: &str| {
            headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(k))
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("Host"), Some("api.xiaoyuzhoufm.com"));
        assert_eq!(
            get("User-Agent"),
            Some("Xiaoyuzhou/2.57.1 (build:1576; iOS 17.4.1)")
        );
        assert_eq!(get("x-jike-access-token"), Some("AT-2"));
        assert_eq!(get("App-Version"), Some("2.57.1"));
        assert_eq!(get("Content-Type"), Some("application/json"));
        assert!(get("Local-Time").is_some());
        assert_eq!(get("Timezone"), Some("Asia/Shanghai"));
    }

    #[test]
    fn 刷新头_包含双token与表单类型() {
        let c = ApiClient::new(DEFAULT_BASE_URL);
        let headers = c.refresh_headers("AT-3", "RT-3");
        let get = |k: &str| {
            headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(k))
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("x-jike-access-token"), Some("AT-3"));
        assert_eq!(get("x-jike-refresh-token"), Some("RT-3"));
        assert_eq!(
            get("Content-Type"),
            Some("application/x-www-form-urlencoded; charset=utf-8")
        );
    }

    #[test]
    fn 播客后台头_使用web_ua() {
        let c = ApiClient::new(DEFAULT_BASE_URL);
        let headers = c.podcaster_headers();
        let ua = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
            .map(|(_, v)| v.as_str());
        assert!(ua.unwrap().starts_with("Mozilla/5.0"));
    }

    #[test]
    fn 本地时间格式_符合rfc3339() {
        let s = local_time_rfc3339();
        // 形如 2026-10-28T14:11:08+08:00
        assert_eq!(s.len(), 25);
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
        assert_eq!(&s[10..11], "T");
        assert!(s.ends_with("+08:00"));
    }
}
