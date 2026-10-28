//! 小宇宙（xyz）API 的 Rust 客户端与数据模型。
//!
//! 本 crate 直连小宇宙上游 API（`https://api.xiaoyuzhoufm.com`），负责：
//! - 封装登录、搜索、订阅、节目/单集详情、评论、播放进度、榜单、收藏等接口；
//! - 定义对应的强类型请求/响应模型；
//! - 上报与官方 App 一致的 iOS 请求头模板（含 `x-jike-access-token` 鉴权头）；
//! - 从上游响应头提取 `x-jike-access-token` / `x-jike-refresh-token`。
//!
//! # 示例
//!
//! ```ignore
//! use stardust_api::{ApiClient, LoginRequest, DEFAULT_BASE_URL};
//!
//! let mut client = ApiClient::new(DEFAULT_BASE_URL);
//! let result = client.login(&LoginRequest {
//!     mobile_phone_number: "13800000000".into(),
//!     verify_code: "1234".into(),
//!     area_code: Some("+86".into()),
//! });
//! if let Ok(login) = result {
//!     if let Some(token) = login.access_token {
//!         client.set_access_token(token);
//!     }
//! }
//! ```

mod client;
mod models;

pub use client::{
    normalize_area_code, unwrap_inner, ApiClient, ApiError, ApiResponse, AuthTokens, Method,
    DEFAULT_BASE_URL,
};
pub use models::*;
