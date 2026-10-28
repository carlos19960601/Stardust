//! xyz API 的 HTTP 客户端：请求封装、响应信封解析与错误处理。
//!
//! xyz 服务所有接口统一返回信封 `{ code, msg, data }`，其中 `code == 200`
//! 表示成功；失败时 `code` 为对应 HTTP 状态码，`msg`/`error` 携带错误信息。
//! 鉴权通过在请求头写入 `x-jike-access-token` 完成。

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;
use thiserror::Error;

use crate::models::*;

/// xyz 服务默认监听地址。
pub const DEFAULT_BASE_URL: &str = "http://localhost:23020";

/// 请求超时时间，对齐 horizon 前端的 15s。
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

/// HTTP 方法。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

/// 通用响应信封。
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

/// xyz API 客户端。
///
/// 内部持有可变的 access token，供会话期间自动附加鉴权头；
/// 需要切换用户或刷新 token 时调用 [`set_access_token`](Self::set_access_token)。
pub struct ApiClient {
    base_url: String,
    access_token: Option<String>,
    agent: ureq::Agent,
}

impl ApiClient {
    /// 创建指向指定地址的客户端（不带 token）。
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            access_token: None,
            agent: ureq::AgentBuilder::new().timeout(DEFAULT_TIMEOUT).build(),
        }
    }

    /// 创建默认地址（`localhost:23020`）的客户端。
    pub fn local() -> Self {
        Self::new(DEFAULT_BASE_URL)
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

    /// 发送请求并解析信封，返回 `data` 字段的原始 JSON（无 `data` 时为 `Null`）。
    pub fn raw(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Value, ApiError> {
        let url = self.url(path);
        let mut req = match method {
            Method::Get => self.agent.get(&url),
            Method::Post => self.agent.post(&url),
        };
        if let Some(token) = &self.access_token {
            req = req.set("x-jike-access-token", token);
        }
        req = req.set("Content-Type", "application/json");

        let resp = match body {
            Some(b) => req.send_json(b),
            None => req.call(),
        };

        let resp = match resp {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                // 状态码非 2xx：尝试读出业务错误信封
                let text = r.into_string().unwrap_or_default();
                if let Ok(env) = serde_json::from_str::<ApiResponse>(&text) {
                    return Err(ApiError::Api {
                        code: env.code,
                        msg: envelope_msg(&env),
                    });
                }
                return Err(ApiError::Http { status: code, body: text });
            }
            Err(ureq::Error::Transport(t)) => return Err(ApiError::Network(t.to_string())),
        };

        let text = resp
            .into_string()
            .map_err(|e| ApiError::Network(e.to_string()))?;
        let env: ApiResponse = serde_json::from_str(&text).map_err(|e| ApiError::Parse {
            target: "响应信封".into(),
            detail: e.to_string(),
        })?;

        if env.code != 200 {
            return Err(ApiError::Api {
                code: env.code,
                msg: envelope_msg(&env),
            });
        }
        Ok(env.data.unwrap_or(Value::Null))
    }

    /// 发送请求并把信封 `data` 直接反序列化为 `T`。
    ///
    /// 适用于 `data` 本身就是目标结构，或目标结构已包含内层 `data`/兄弟字段
    /// （如 [`LoginResult`]、[`SearchResponse`]）的场景。
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
    ///
    /// 适用于详情、列表、榜单等「信封 data 为 `{ data: T }`」的接口，
    /// 同时对「直接返回 T」的形态做了兼容。
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

    // -----------------------------------------------------------------------
    // 鉴权
    // -----------------------------------------------------------------------

    /// 发送短信验证码。
    pub fn send_code(&self, req: &SendCodeRequest) -> Result<Value, ApiError> {
        self.post_raw("sendCode", serde_json::to_value(req).unwrap_or(Value::Null))
    }

    /// 验证码登录，成功后返回用户信息与 token。
    pub fn login(&self, req: &LoginRequest) -> Result<LoginResult, ApiError> {
        self.post("login", serde_json::to_value(req).unwrap_or(Value::Null))
    }

    /// 刷新 token。
    pub fn refresh_token(&self, req: &RefreshTokenRequest) -> Result<RefreshTokenData, ApiError> {
        self.post(
            "refresh_token",
            serde_json::to_value(req).unwrap_or(Value::Null),
        )
    }

    // -----------------------------------------------------------------------
    // 搜索
    // -----------------------------------------------------------------------

    /// 搜索节目/单集/用户。
    pub fn search(&self, req: &SearchRequest) -> Result<SearchResponse, ApiError> {
        self.post("search", serde_json::to_value(req).unwrap_or(Value::Null))
    }

    /// 「你可能想搜的内容」。
    pub fn search_preset(&self) -> Result<Value, ApiError> {
        self.post_raw("search_preset", Value::Object(Default::default()))
    }

    // -----------------------------------------------------------------------
    // 单集
    // -----------------------------------------------------------------------

    /// 查询节目内的单集列表。
    pub fn episode_list(&self, req: &EpisodeListRequest) -> Result<EpisodeListResponse, ApiError> {
        self.post(
            "episode_list",
            serde_json::to_value(req).unwrap_or(Value::Null),
        )
    }

    /// 节目内「最受欢迎」单集列表。
    pub fn episode_list_by_filter(&self, pid: &str) -> Result<EpisodeListResponse, ApiError> {
        self.post(
            "episode_list_by_filter",
            serde_json::json!({ "pid": pid }),
        )
    }

    /// 查询单集详情。
    pub fn episode_detail(&self, eid: &str) -> Result<Episode, ApiError> {
        self.post_inner("episode_detail", serde_json::json!({ "eid": eid }))
    }

    /// 正在收听该单集的人数。
    pub fn episode_live_count(&self, eid: &str) -> Result<Value, ApiError> {
        self.post_inner("episode_live_count", serde_json::json!({ "eid": eid }))
    }

    /// 查询单集播放进度。
    pub fn episode_play_progress(&self, eids: &[String]) -> Result<Vec<PlaybackProgressEntry>, ApiError> {
        self.post_inner(
            "episode_play_progress",
            serde_json::json!({ "eids": eids }),
        )
    }

    /// 更新单集播放进度。
    pub fn episode_play_progress_update(
        &self,
        data: &[PlaybackProgressItem],
    ) -> Result<Value, ApiError> {
        self.post_inner(
            "episode_play_progress_update",
            serde_json::json!({ "data": data }),
        )
    }

    /// 上报播放状态。
    pub fn live_stats_report(&self, req: &LiveStatsReportRequest) -> Result<Value, ApiError> {
        self.post_inner(
            "live_stats_report",
            serde_json::to_value(req).unwrap_or(Value::Null),
        )
    }

    /// 精彩时间点。
    pub fn episode_clap(&self, eid: &str) -> Result<Value, ApiError> {
        self.post_inner("episode_clap", serde_json::json!({ "eid": eid }))
    }

    /// 标记精彩时间点。
    pub fn episode_clap_create(
        &self,
        eid: &str,
        timestamp: i64,
        duration: i64,
    ) -> Result<Value, ApiError> {
        self.post_inner(
            "episode_clap_create",
            serde_json::json!({ "eid": eid, "timestamp": timestamp, "duration": duration }),
        )
    }

    /// 根据 uid 查询用户的收听历史记录。
    pub fn played_list(&self, uid: &str) -> Result<Value, ApiError> {
        self.post_inner("played_list", serde_json::json!({ "uid": uid }))
    }

    // -----------------------------------------------------------------------
    // 节目
    // -----------------------------------------------------------------------

    /// 查询节目详情。
    pub fn podcast_detail(&self, pid: &str) -> Result<Podcast, ApiError> {
        self.post_inner("podcast_detail", serde_json::json!({ "pid": pid }))
    }

    /// 获取节目主体信息。
    pub fn podcast_get_info(&self, pid: &str) -> Result<Podcast, ApiError> {
        self.post_inner("podcast_get_info", serde_json::json!({ "pid": pid }))
    }

    /// 获取节目荣誉墙。
    pub fn podcast_honor_list(&self, pid: &str) -> Result<Vec<PodcastHonor>, ApiError> {
        self.post_inner("podcast_honor_list", serde_json::json!({ "pid": pid }))
    }

    /// 相关节目推荐。
    pub fn podcast_related(&self, pid: &str) -> Result<Value, ApiError> {
        self.post_inner("podcast_related", serde_json::json!({ "pid": pid }))
    }

    /// 获取节目公告。
    pub fn podcast_bulletin(&self, pid: &str) -> Result<PodcastBulletin, ApiError> {
        self.post_inner("podcast_bulletin", serde_json::json!({ "pid": pid }))
    }

    /// 查询用户创建的播客节目。
    pub fn owned_podcasts(&self, uid: &str) -> Result<Vec<Podcast>, ApiError> {
        self.post_inner("owned_podcasts", serde_json::json!({ "uid": uid }))
    }

    // -----------------------------------------------------------------------
    // 订阅
    // -----------------------------------------------------------------------

    /// 我的订阅列表。
    pub fn subscription(&self, req: &SubscriptionRequest) -> Result<SubscriptionResponse, ApiError> {
        self.post(
            "subscription",
            serde_json::to_value(req).unwrap_or(Value::Null),
        )
    }

    /// 更新订阅状态。
    pub fn subscription_update(&self, req: &SubscriptionUpdateRequest) -> Result<Value, ApiError> {
        self.post_inner(
            "subscription_update",
            serde_json::to_value(req).unwrap_or(Value::Null),
        )
    }

    /// 星标订阅列表。
    pub fn subscription_star(&self) -> Result<Value, ApiError> {
        self.post_raw("subscription_star", Value::Object(Default::default()))
    }

    /// 未加星标订阅列表。
    pub fn subscription_non_starred(&self) -> Result<Value, ApiError> {
        self.post_raw(
            "subscription_non_starred",
            Value::Object(Default::default()),
        )
    }

    /// 更新星标订阅。
    pub fn subscription_star_update(&self, body: Value) -> Result<Value, ApiError> {
        self.post_raw("subscription_star_update", body)
    }

    // -----------------------------------------------------------------------
    // 评论
    // -----------------------------------------------------------------------

    /// 查询单集的评论。
    pub fn comment_primary(&self, body: Value) -> Result<CommentListResponse, ApiError> {
        self.post("comment_primary", body)
    }

    /// 查询回复评论。
    pub fn comment_thread(&self, body: Value) -> Result<CommentListResponse, ApiError> {
        self.post("comment_thread", body)
    }

    /// 收藏评论。
    pub fn comment_collect_create(&self, body: Value) -> Result<Value, ApiError> {
        self.post_raw("comment_collect_create", body)
    }

    /// 取消收藏评论。
    pub fn comment_collect_remove(&self, body: Value) -> Result<Value, ApiError> {
        self.post_raw("comment_collect_remove", body)
    }

    /// 获取收藏评论列表。
    pub fn comment_collect_list(&self, body: Value) -> Result<Value, ApiError> {
        self.post_raw("comment_collect_list", body)
    }

    /// 点赞/取消点赞评论。
    pub fn comment_like_update(&self, body: Value) -> Result<Value, ApiError> {
        self.post_raw("comment_like_update", body)
    }

    /// 创建评论。
    pub fn comment_create(&self, body: Value) -> Result<Value, ApiError> {
        self.post_raw("comment_create", body)
    }

    /// 删除评论。
    pub fn comment_remove(&self, body: Value) -> Result<Value, ApiError> {
        self.post_raw("comment_remove", body)
    }

    // -----------------------------------------------------------------------
    // 首页发现 / 榜单
    // -----------------------------------------------------------------------

    /// 首页推荐、精选、榜单等模块。
    pub fn discovery(&self, load_more_key: Option<&str>) -> Result<DiscoveryResponse, ApiError> {
        let body = match load_more_key {
            Some(key) => serde_json::json!({ "loadMoreKey": key }),
            None => serde_json::json!({}),
        };
        self.post("discovery", body)
    }

    /// 获取最热榜/锋芒榜/新星榜。
    pub fn top_list(&self, category: &str) -> Result<TopList, ApiError> {
        self.post_inner(
            "top_list",
            serde_json::json!({ "category": category }),
        )
    }

    /// 编辑精选-往日精选历史。
    pub fn editor_pick_list_history(&self, body: Value) -> Result<Value, ApiError> {
        self.post_raw("editor_pick_list_history", body)
    }

    /// 新节目广场。
    pub fn pilot_discovery_list(&self, body: Value) -> Result<Value, ApiError> {
        self.post_raw("pilot_discovery_list", body)
    }

    /// 首页「大家都在听」刷新推荐。
    pub fn refresh_episode_recommend(&self, body: Value) -> Result<Value, ApiError> {
        self.post_raw("refresh_episode_recommend", body)
    }

    // -----------------------------------------------------------------------
    // 用户 / 个人资料
    // -----------------------------------------------------------------------

    /// 根据 uid 查询用户信息。
    pub fn profile(&self, uid: &str) -> Result<BaseUser, ApiError> {
        self.post_inner("profile", serde_json::json!({ "uid": uid }))
    }

    /// 根据 uid 查询用户信息（get_profile 别名）。
    pub fn get_profile(&self, uid: &str) -> Result<BaseUser, ApiError> {
        self.post_inner("get_profile", serde_json::json!({ "uid": uid }))
    }

    /// 用户统计数据。
    pub fn user_stats(&self, uid: &str) -> Result<UserStats, ApiError> {
        self.post_inner("user_stats", serde_json::json!({ "uid": uid }))
    }

    /// 根据 uid 查询已获得的贴纸。
    pub fn sticker(&self, uid: &str) -> Result<Value, ApiError> {
        self.post_inner("sticker", serde_json::json!({ "uid": uid }))
    }

    /// 查询我的贴纸墙。
    pub fn sticker_board(&self, uid: &str) -> Result<Value, ApiError> {
        self.post_inner("sticker_board", serde_json::json!({ "uid": uid }))
    }

    // -----------------------------------------------------------------------
    // 收藏 / 关注 / 历史 / 设置
    // -----------------------------------------------------------------------

    /// 更新收藏单集。
    pub fn favorite_episode_update(&self, eid: &str) -> Result<Value, ApiError> {
        self.post_raw("favorite_episode_update", serde_json::json!({ "eid": eid }))
    }

    /// 获取收藏单集列表。
    pub fn favorite_episode_list(&self, body: Value) -> Result<Value, ApiError> {
        self.post_raw("favorite_episode_list", body)
    }

    /// 收听历史。
    pub fn episode_played_history_list(&self, body: Value) -> Result<Value, ApiError> {
        self.post_raw("episode_played_history_list", body)
    }

    /// 更新收听历史。
    pub fn episode_played_history_list_update(&self, body: Value) -> Result<Value, ApiError> {
        self.post_raw("episode_played_history_list_update", body)
    }

    /// 订阅更新列表。
    pub fn inbox_list(&self, body: Value) -> Result<Value, ApiError> {
        self.post_raw("inbox_list", body)
    }

    /// 未读消息数。
    pub fn unread_count(&self) -> Result<Value, ApiError> {
        self.post_raw("unread_count", Value::Object(Default::default()))
    }

    /// 获取「我」关注的人。
    pub fn following_list(&self, uid: &str) -> Result<Vec<BaseUser>, ApiError> {
        self.post_inner("following_list", serde_json::json!({ "uid": uid }))
    }

    /// 获取关注「我」的人。
    pub fn follower_list(&self, uid: &str) -> Result<Vec<BaseUser>, ApiError> {
        self.post_inner("follower_list", serde_json::json!({ "uid": uid }))
    }

    /// 关注/取关用户。
    pub fn relation_update(&self, body: Value) -> Result<Value, ApiError> {
        self.post_raw("relation_update", body)
    }

    /// 获取用户偏好设置。
    pub fn user_preference_get(&self, body: Value) -> Result<Value, ApiError> {
        self.post_raw("user_preference_get", body)
    }

    /// 更新用户偏好设置。
    pub fn user_preference_update(&self, body: Value) -> Result<Value, ApiError> {
        self.post_raw("user_preference_update", body)
    }

    /// 获取付费单集音频链接。
    pub fn private_media_get(&self, body: Value) -> Result<Value, ApiError> {
        self.post_raw("private_media_get", body)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn 解包内层data_兼容双重与单层() {
        // 双重包裹：{ data: { ... } } -> 取出内层
        let v = unwrap_inner(json!({ "data": { "eid": "e1" } }));
        assert_eq!(v, json!({ "eid": "e1" }));

        // 直接对象：无 data 字段 -> 原样返回
        let v = unwrap_inner(json!({ "eid": "e1" }));
        assert_eq!(v, json!({ "eid": "e1" }));

        // 直接数组：数组无 key -> 原样返回
        let v = unwrap_inner(json!([1, 2, 3]));
        assert_eq!(v, json!([1, 2, 3]));

        // 内层 data 为 null
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
        let c = ApiClient::new("http://localhost:23020/");
        assert_eq!(c.url("/ping"), "http://localhost:23020/ping");
        assert_eq!(c.url("ping"), "http://localhost:23020/ping");
        let c2 = ApiClient::new("http://localhost:23020");
        assert_eq!(c2.url("/search"), "http://localhost:23020/search");
    }

    #[test]
    fn token读写() {
        let mut c = ApiClient::local();
        assert_eq!(c.access_token(), None);
        c.set_access_token("tok");
        assert_eq!(c.access_token(), Some("tok"));
        c.clear_access_token();
        assert_eq!(c.access_token(), None);
    }
}
