//! 小宇宙（xyz）API 的 Rust 客户端与数据模型。
//!
//! 内嵌的 xyz 服务默认运行在 `localhost:23020`，本 crate 负责：
//! - 封装登录、搜索、订阅、节目/单集详情、评论、播放进度、榜单、收藏等接口；
//! - 定义对应的强类型请求/响应模型；
//! - 统一处理 `{ code, msg, data }` 响应信封与错误。
//!
//! # 示例
//!
//! ```ignore
//! use stardust_api::{ApiClient, LoginRequest};
//!
//! let mut client = ApiClient::local();
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
    unwrap_inner, ApiClient, ApiError, ApiResponse, Method, DEFAULT_BASE_URL,
};
pub use models::*;
