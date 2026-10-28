//! 在线冒烟测试：针对真实的小宇宙上游 API 验证客户端（需要真实账号/令牌）。
//!
//! 默认 `#[ignore]`，需手动设置令牌后运行：
//!
//! ```bash
//! cargo test -p stardust-api --test live_smoke -- --ignored --nocapture
//! ```

use stardust_api::{ApiClient, ApiError, Method, DEFAULT_BASE_URL};

/// `/ping` 无需鉴权，可验证 HTTP 层与信封解析。
///
/// 注意：`/ping` 返回的是 `{ code:200, message:"pong", version:... }`，
/// 没有 `data` 字段，因此 `raw` 成功返回 `Null`，同时 `msg` 被别名 `message` 填充。
#[test]
#[ignore]
fn ping_连通并解析信封() {
    let client = ApiClient::new(DEFAULT_BASE_URL);
    let data = client
        .raw(Method::Get, "ping", None)
        .expect("调用 /ping 失败");
    assert!(data.is_null());
    println!("/ping 信封解析成功：code=200，data 为空");
}

/// 受保护接口在缺少 token 时返回 400 业务错误信封，应映射为 `ApiError::Api`。
#[test]
#[ignore]
fn 无token访问受保护接口返回业务错误() {
    let client = ApiClient::new(DEFAULT_BASE_URL);
    let err = client
        .search_preset()
        .expect_err("缺少 token 时应报错");
    match err {
        ApiError::Api { code, msg } => {
            assert_eq!(code, 400);
            assert!(!msg.is_empty());
            println!("业务错误信封解析成功：code={code}, msg={msg}");
        }
        other => panic!("期望 ApiError::Api，实际为 {other:?}"),
    }
}

/// 刷新 token 缺参时应返回 400 业务错误（无需有效 token）。
#[test]
#[ignore]
fn 刷新token缺参返回业务错误() {
    let client = ApiClient::new(DEFAULT_BASE_URL);
    let err = client
        .post_raw("refresh_token", serde_json::json!({}))
        .expect_err("缺参时应报错");
    match err {
        ApiError::Api { code, .. } => assert_eq!(code, 400),
        other => panic!("期望 ApiError::Api，实际为 {other:?}"),
    }
}
