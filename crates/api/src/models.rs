//! 数据模型：小宇宙（xyz）API 的强类型请求/响应结构。
//!
//! 上游 JSON 采用 camelCase 命名，这里统一通过 `rename_all = "camelCase"` 映射；
//! 字段一律声明为 `Option` 并叠加 `#[serde(default)]` 宽容处理，避免上游新增或
//! 裁剪字段导致整体反序列化失败。未知字段默认被 serde 忽略。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 上游响应常见的内层 `data` 包裹（形如 `{ "data": ... }`）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DataWrap<T> {
    pub data: Option<T>,
}

/// 图片信息（节目封面、单集封面、头像等，含多尺寸地址）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Image {
    pub format: Option<String>,
    pub height: Option<i64>,
    pub width: Option<i64>,
    pub pic_url: Option<String>,
    pub large_pic_url: Option<String>,
    pub middle_pic_url: Option<String>,
    pub small_pic_url: Option<String>,
    /// 上游字段为 `thumbnailUrl`，历史类型里曾出现拼写 `thumbnaiUrl`，一并兼容。
    #[serde(alias = "thumbnaiUrl")]
    pub thumbnail_url: Option<String>,
}

/// 音频资源来源。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MediaSource {
    pub mode: Option<String>,
    pub url: Option<String>,
}

/// 音频资源（大小、链接等）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Media {
    pub id: Option<String>,
    pub mime_type: Option<String>,
    pub size: Option<i64>,
    pub source: Option<MediaSource>,
}

/// 主题色。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Color {
    pub dark: Option<String>,
    pub light: Option<String>,
    pub original: Option<String>,
}

/// 联系方式（公众号、微博、邮箱等）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Contact {
    pub name: Option<String>,
    pub note: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub url: Option<String>,
}

/// 头像（内含多尺寸图片）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Avatar {
    pub picture: Option<Image>,
}

/// 用户基础信息（主播、评论作者等均使用此结构）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BaseUser {
    pub uid: String,
    pub nickname: Option<String>,
    pub avatar: Option<Avatar>,
    pub bio: Option<String>,
    pub gender: Option<String>,
    pub ip_loc: Option<String>,
    pub relation: Option<String>,
    pub is_nickname_set: Option<bool>,
    pub is_cancelled: Option<bool>,
    pub is_blocked_by_viewer: Option<bool>,
    pub is_invited: Option<bool>,
}

/// 登录成功后返回的用户信息。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct User {
    pub uid: String,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub nickname: Option<String>,
    pub avatar: Option<Avatar>,
    pub bio: Option<String>,
    pub gender: Option<String>,
    pub industry: Option<String>,
    pub ip_loc: Option<String>,
    pub birth_year: Option<i64>,
    pub is_nickname_set: Option<bool>,
    pub is_cancelled: Option<bool>,
    pub is_blocked_by_viewer: Option<bool>,
    pub phone_number: Option<PhoneNumber>,
    pub phone_number_needed: Option<bool>,
    pub wechat_user_info: Option<WechatUserInfo>,
    pub jike_user_info: Option<JikeUserInfo>,
}

/// 手机号信息。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PhoneNumber {
    pub mobile_phone_number: Option<String>,
    pub area_code: Option<String>,
}

/// 绑定的微信信息。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WechatUserInfo {
    pub nick_name: Option<String>,
}

/// 绑定的即刻信息。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct JikeUserInfo {
    pub nickname: Option<String>,
}

/// 用户统计数据（关注/粉丝/订阅/累计收听秒数）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UserStats {
    pub follower_count: i64,
    pub following_count: i64,
    pub subscription_count: i64,
    pub total_played_seconds: i64,
}

/// 节目（播客）信息。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Podcast {
    pub pid: String,
    pub title: Option<String>,
    pub author: Option<String>,
    pub brief: Option<String>,
    pub description: Option<String>,
    pub image: Option<Image>,
    pub color: Option<Color>,
    #[serde(default)]
    pub contacts: Vec<Contact>,
    pub episode_count: i64,
    pub has_popular_episodes: Option<bool>,
    pub latest_episode_pub_date: Option<String>,
    pub pay_episode_count: i64,
    pub pay_type: Option<String>,
    pub subscription_count: i64,
    pub subscription_push: Option<bool>,
    pub subscription_push_priority: Option<String>,
    pub subscription_star: Option<bool>,
    pub subscription_status: Option<String>,
    pub sync_mode: Option<String>,
    pub status: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub podcasters: Vec<BaseUser>,
    pub permissions: Option<Value>,
    pub read_track_info: Option<Value>,
    pub topic_labels: Option<Value>,
    pub is_customized: Option<bool>,
}

/// 节目公告。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PodcastBulletin {
    pub id: Option<String>,
    pub content: Option<String>,
    pub created_at: Option<String>,
    pub offline: Option<bool>,
    #[serde(default)]
    pub pictures: Vec<Image>,
    pub podcast: Option<Podcast>,
    #[serde(default)]
    pub reactions: Vec<Reaction>,
    pub unique_visitor_count: i64,
}

/// 节目荣誉。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PodcastHonor {
    pub id: Option<String>,
    pub campaign_title: Option<String>,
    pub title: Option<String>,
    pub url: Option<String>,
}

/// 单集音频外链。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Enclosure {
    pub url: Option<String>,
}

/// 试听片段信息。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Trial {
    pub from: i64,
    pub full_duration: i64,
    pub segment: Option<String>,
    pub to: i64,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

/// 单集（剧集）信息。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Episode {
    pub eid: String,
    pub pid: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub shownotes: Option<Value>,
    pub duration: i64,
    pub media: Option<Media>,
    pub enclosure: Option<Enclosure>,
    pub media_key: Option<String>,
    pub image: Option<Image>,
    pub podcast: Option<Podcast>,
    pub clap_count: i64,
    pub comment_count: i64,
    pub favorite_count: i64,
    pub play_count: i64,
    pub pub_date: Option<String>,
    pub ip_loc: Option<String>,
    pub pay_type: Option<String>,
    pub status: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub is_customized: Option<bool>,
    pub is_favorited: Option<bool>,
    pub is_finished: Option<bool>,
    pub is_picked: Option<bool>,
    pub is_played: Option<bool>,
    pub is_private_media: Option<bool>,
    pub is_owned: Option<bool>,
    pub labels: Option<Value>,
    pub permissions: Option<Value>,
    pub sponsors: Option<Value>,
    pub read_track_info: Option<Value>,
    pub wechat_share: Option<Value>,
    pub trial: Option<Trial>,
}

/// 互动（点赞等）信息。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Reaction {
    pub count: i64,
    pub name: Option<String>,
    pub reacted: Option<bool>,
    pub url: Option<String>,
}

/// 语音评论。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Voice {
    pub duration: i64,
    pub url: Option<String>,
    pub waveform: Option<Value>,
}

/// 评论所属对象。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CommentOwner {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

/// 评论（一级评论 / 回复）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Comment {
    pub id: Option<String>,
    pub pid: Option<String>,
    pub author: Option<BaseUser>,
    pub episode: Option<Episode>,
    pub text: Option<String>,
    pub voice: Option<Voice>,
    pub owner: Option<CommentOwner>,
    pub like_count: i64,
    pub liked: Option<bool>,
    pub collected: Option<bool>,
    pub collected_at: Option<String>,
    pub created_at: Option<String>,
    pub ip_loc: Option<String>,
    pub is_author_muted: Option<bool>,
    pub level: i64,
    pub pinned: Option<bool>,
    pub status: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub author_association: Option<String>,
    pub reply_count: i64,
    pub thread_reply_count: i64,
    pub badges: Option<Value>,
    pub reply_to_comment: Option<Box<Comment>>,
    pub replies: Option<Value>,
}

/// 搜索结果条目（节目 / 单集 / 用户 的联合体，具体字段按 `type` 区分）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchItem {
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub pid: Option<String>,
    pub eid: Option<String>,
    pub uid: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub shownotes: Option<String>,
    pub media: Option<Media>,
    pub image: Option<Image>,
    pub podcasters: Option<Vec<BaseUser>>,
    pub load_more_key: Option<Value>,
    /// 兜底字段，保留未显式建模的字段，便于按 `type` 二次解析。
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

// ---------------------------------------------------------------------------
// 请求结构
// ---------------------------------------------------------------------------

/// 发送短信验证码。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendCodeRequest {
    pub mobile_phone_number: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub area_code: Option<String>,
}

/// 验证码登录。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    pub mobile_phone_number: String,
    pub verify_code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub area_code: Option<String>,
}

/// 刷新 token。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RefreshTokenRequest {
    #[serde(rename = "x-jike-access-token")]
    pub x_jike_access_token: String,
    #[serde(rename = "x-jike-refresh-token")]
    pub x_jike_refresh_token: String,
}

/// 搜索。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub keyword: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_more_key: Option<Value>,
}

/// 单集列表。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeListRequest {
    pub pid: String,
    pub order: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_more_key: Option<Value>,
}

/// 查询单集详情。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeDetailRequest {
    pub eid: String,
}

/// 查询节目详情。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PodcastDetailRequest {
    pub pid: String,
}

/// 查询单集播放进度。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackProgressRequest {
    pub eids: Vec<String>,
}

/// 播放进度条目。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlaybackProgressItem {
    pub pid: String,
    pub eid: String,
    pub progress: i64,
    pub played_at: String,
}

/// 更新单集播放进度。
#[derive(Debug, Clone, Default, Serialize)]
pub struct PlaybackProgressUpdateRequest {
    pub data: Vec<PlaybackProgressItem>,
}

/// 上报播放状态。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStatsReportRequest {
    pub eid: String,
    pub pid: String,
}

/// 榜单查询。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopListRequest {
    pub category: String,
}

/// 订阅列表查询。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_more_key: Option<Value>,
}

/// 更新订阅状态。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionUpdateRequest {
    pub pid: String,
    pub status: String,
}

// ---------------------------------------------------------------------------
// 响应结构（部分接口存在内层 `data` 包裹 + 兄弟字段，如 loadMoreKey）
// ---------------------------------------------------------------------------

/// 登录结果：信封 `data` 字段里同时包含用户与两个 token。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct LoginResult {
    #[serde(rename = "x-jike-access-token", default)]
    pub access_token: Option<String>,
    #[serde(rename = "x-jike-refresh-token", default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub data: Option<User>,
}

/// 刷新 token 结果。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RefreshTokenData {
    #[serde(rename = "x-jike-access-token", default)]
    pub access_token: Option<String>,
    #[serde(rename = "x-jike-refresh-token", default)]
    pub refresh_token: Option<String>,
}

/// 搜索结果。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchResponse {
    #[serde(default)]
    pub data: Vec<SearchItem>,
    pub load_more_key: Option<Value>,
}

/// 单集列表结果。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EpisodeListResponse {
    #[serde(default)]
    pub data: Vec<Episode>,
    pub load_more_key: Option<Value>,
}

/// 订阅列表结果。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SubscriptionResponse {
    #[serde(default)]
    pub data: Vec<Podcast>,
    pub load_more_key: Option<Value>,
}

/// 评论列表结果。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CommentListResponse {
    #[serde(default)]
    pub data: Vec<Comment>,
    pub load_more_key: Option<Value>,
}

/// 播放进度查询结果条目。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlaybackProgressEntry {
    pub eid: String,
    pub pid: Option<String>,
    pub played_at: Option<String>,
    pub progress: i64,
}

/// 首页发现/榜单的单个板块。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DiscoveryBlock {
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub title: Option<String>,
    pub data: Option<Value>,
}

/// 首页发现结果。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DiscoveryResponse {
    #[serde(default)]
    pub data: Vec<DiscoveryBlock>,
}

/// 榜单详情。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TopList {
    pub background: Option<String>,
    pub information: Option<String>,
    pub items: Option<Value>,
    pub publish_date: Option<String>,
    pub rules_url: Option<String>,
    pub title: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn 解析单集详情_双重包裹与核心字段() {
        let raw = json!({
            "data": {
                "eid": "662b664a8a089719b7f6bbd3",
                "pid": "61791d921989541784257779",
                "title": "第 100 期",
                "duration": 6101,
                "media": {
                    "id": "x.m4a",
                    "mimeType": "audio/mp4",
                    "size": 98774871,
                    "source": { "mode": "PUBLIC", "url": "https://media.xyzcdn.net/x.m4a" }
                },
                "image": {
                    "picUrl": "https://x.png",
                    "thumbnailUrl": "https://x.png@thumbnail"
                },
                "clapCount": 753,
                "commentCount": 1168,
                "isPlayed": false,
                "type": "EPISODE"
            }
        });
        let wrap: DataWrap<Episode> = serde_json::from_value(raw).unwrap();
        let ep = wrap.data.unwrap();
        assert_eq!(ep.eid, "662b664a8a089719b7f6bbd3");
        assert_eq!(ep.pid.as_deref(), Some("61791d921989541784257779"));
        assert_eq!(ep.title.as_deref(), Some("第 100 期"));
        assert_eq!(ep.duration, 6101);
        // 注意：媒体字段为 mimeType（camelCase），不是 mimetype
        let media = ep.media.unwrap();
        assert_eq!(media.mime_type.as_deref(), Some("audio/mp4"));
        assert_eq!(media.source.unwrap().url.as_deref(), Some("https://media.xyzcdn.net/x.m4a"));
        assert_eq!(ep.image.unwrap().thumbnail_url.as_deref(), Some("https://x.png@thumbnail"));
        assert_eq!(ep.clap_count, 753);
        assert_eq!(ep.is_played, Some(false));
        assert_eq!(ep.kind.as_deref(), Some("EPISODE"));
    }

    #[test]
    fn 解析节目详情_嵌套与主题色() {
        let raw = json!({
            "data": {
                "pid": "61791d921989541784257779",
                "title": "不开玩笑",
                "author": "不开玩笑小助手",
                "episodeCount": 136,
                "hasPopularEpisodes": true,
                "subscriptionCount": 100,
                "color": { "dark": "#0B0B0B", "light": "#ACACAC", "original": "#1A1A1A" },
                "contacts": [
                    { "name": "猫头鹰喜剧", "type": "wechatOfficialAccounts" }
                ]
            }
        });
        let wrap: DataWrap<Podcast> = serde_json::from_value(raw).unwrap();
        let p = wrap.data.unwrap();
        assert_eq!(p.pid, "61791d921989541784257779");
        assert_eq!(p.title.as_deref(), Some("不开玩笑"));
        assert_eq!(p.episode_count, 136);
        assert_eq!(p.color.unwrap().original.as_deref(), Some("#1A1A1A"));
        assert_eq!(p.contacts.len(), 1);
        assert_eq!(p.contacts[0].kind.as_deref(), Some("wechatOfficialAccounts"));
    }

    #[test]
    fn 解析登录结果_用户与令牌() {
        let raw = json!({
            "data": {
                "uid": "123",
                "nickname": "测试用户",
                "phoneNumber": { "mobilePhoneNumber": "13800000000", "areaCode": "+86" }
            },
            "x-jike-access-token": "AT-xxx",
            "x-jike-refresh-token": "RT-xxx"
        });
        let r: LoginResult = serde_json::from_value(raw).unwrap();
        assert_eq!(r.access_token.as_deref(), Some("AT-xxx"));
        assert_eq!(r.refresh_token.as_deref(), Some("RT-xxx"));
        let user = r.data.unwrap();
        assert_eq!(user.uid, "123");
        assert_eq!(user.phone_number.unwrap().mobile_phone_number.as_deref(), Some("13800000000"));
    }

    #[test]
    fn 解析搜索结果_条目与分页() {
        let raw = json!({
            "data": [
                { "type": "PODCAST", "pid": "p1", "title": "节目A" },
                { "type": "EPISODE", "eid": "e1", "title": "单集B", "extraField": 42 }
            ],
            "loadMoreKey": { "loadMoreKey": 20, "searchId": "s-1" }
        });
        let r: SearchResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(r.data.len(), 2);
        assert_eq!(r.data[0].kind.as_deref(), Some("PODCAST"));
        assert_eq!(r.data[1].eid.as_deref(), Some("e1"));
        // 未建模字段落入 extra
        assert_eq!(r.data[1].extra.get("extraField").and_then(Value::as_i64), Some(42));
        assert!(r.load_more_key.is_some());
    }

    #[test]
    fn 解析错误信封_字段宽容() {
        // 错误时 data 可能是字符串
        let raw = json!({ "code": 400, "msg": "错误请求", "data": "some error" });
        let env: crate::client::ApiResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(env.code, 400);
        assert_eq!(env.msg, "错误请求");
    }

    #[test]
    fn 字段缺失时按默认值处理() {
        // 只有 eid 的单集也能解析成功，其余字段取默认值
        let ep: Episode = serde_json::from_value(json!({ "eid": "e1" })).unwrap();
        assert_eq!(ep.eid, "e1");
        assert_eq!(ep.duration, 0);
        assert!(ep.title.is_none());
    }
}
