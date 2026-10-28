//! Golden 单测：断言全部业务接口（sendCode / login / refresh_token 及 64 个业务
//! 接口）的上游 URL、请求头与请求体与官方 iOS 客户端行为完全一致。
//!
//! 通过本地一次性 TCP 假服务器捕获客户端发出的原始 HTTP 请求，再与固化在本文件
//! 头部的 URL/headers/body 基准常量逐项对照，避免真实联网。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

use serde_json::Value;
use stardust_api::{
    ApiClient, CaptchaPayload, CommentPrimaryLoadMoreKey, EpisodeListRequest, InboxLoadMoreKey,
    LiveStatsReportRequest, LoginRequest, MileageTracking, PlaybackProgressItem,
    RefreshTokenRequest, SearchRequest, SendCodeRequest, SubscriptionRequest,
    SubscriptionUpdateRequest,
};

/// 捕获到的原始 HTTP 请求。
struct CapturedRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

/// 读取一次完整的 HTTP 请求（按 Content-Length 精确截取 body）。
fn read_http_request(stream: &mut TcpStream) -> CapturedRequest {
    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 8192];
    let header_end;
    loop {
        let n = stream.read(&mut tmp).expect("读取请求失败");
        assert!(n > 0, "连接被对端提前关闭");
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            header_end = pos + 4;
            break;
        }
    }

    let header_text = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();

    let mut headers = Vec::new();
    let mut content_length = 0usize;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim().to_string();
            let v = v.trim().to_string();
            if k.eq_ignore_ascii_case("content-length") {
                content_length = v.parse().unwrap_or(0);
            }
            headers.push((k, v));
        }
    }

    let mut body = buf[header_end..].to_vec();
    while body.len() < content_length {
        let mut tmp = [0u8; 8192];
        let n = stream.read(&mut tmp).expect("读取 body 失败");
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(content_length);
    let body = String::from_utf8_lossy(&body).to_string();

    CapturedRequest {
        method,
        path,
        headers,
        body,
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

/// 写出一个 200 OK 响应（附带指定响应头与 JSON body）。
fn write_response(
    stream: &mut TcpStream,
    body: &str,
    headers: &[(&str, &str)],
) {
    let mut resp = String::from("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n");
    for (k, v) in headers {
        resp.push_str(&format!("{k}: {v}\r\n"));
    }
    resp.push_str(&format!("Content-Length: {}\r\n", body.len()));
    resp.push_str("Connection: close\r\n\r\n");
    resp.push_str(body);
    stream.write_all(resp.as_bytes()).expect("写响应失败");
    stream.flush().ok();
}

/// 启动一次性假服务器，`run` 内同步调用客户端，随后返回捕获到的请求。
fn mock_once<F>(response_body: &'static str, response_headers: &'static [(&'static str, &'static str)], run: F) -> CapturedRequest
where
    F: FnOnce(String),
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("绑定假服务器失败");
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");

    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("接受连接失败");
        let captured = read_http_request(&mut stream);
        write_response(&mut stream, response_body, response_headers);
        captured
    });

    run(base.clone());
    handle.join().expect("假服务器线程 panic")
}

/// 大小写不敏感地取出某个请求头。
fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn assert_header(headers: &[(String, String)], name: &str, expected: &str) {
    let actual = header_value(headers, name).unwrap_or_else(|| panic!("缺少请求头 {name}"));
    assert_eq!(actual, expected, "请求头 {name} 不一致");
}

/// JSON 语义等价比较（忽略键顺序，Go 的 `json.Marshal(map)` 会按键名排序）。
fn json_eq(a: &str, b: &str) -> bool {
    let va: Value = serde_json::from_str(a).unwrap_or(Value::Null);
    let vb: Value = serde_json::from_str(b).unwrap_or(Value::Null);
    va == vb
}

const PODCASTER_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/146.0.0.0 Safari/537.36 Edg/146.0.0.0";

// ---------------------------------------------------------------------------
// sendCode
// ---------------------------------------------------------------------------

#[test]
fn 发送验证码_golden() {
    let captured = mock_once(
        r#"{"code":200,"msg":"OK","data":null}"#,
        &[],
        |base| {
            let client = ApiClient::with_bases(base.clone(), base);
            client
                .send_code(&SendCodeRequest {
                    mobile_phone_number: "13800000000".into(),
                    area_code: Some("+86".into()),
                    captcha: CaptchaPayload::web("verify-param-123"),
                })
                .unwrap();
        },
    );

    assert_eq!(captured.method, "POST");
    assert_eq!(captured.path, "/v1/auth/send-code");
    assert_header(&captured.headers, "accept", "application/json, text/plain, */*");
    assert_header(
        &captured.headers,
        "accept-language",
        "zh-CN,zh;q=0.9,en;q=0.8,en-GB;q=0.7,en-US;q=0.6",
    );
    assert_header(&captured.headers, "content-type", "application/json;charset=UTF-8");
    assert_header(&captured.headers, "origin", "https://podcaster.xiaoyuzhoufm.com");
    assert_header(&captured.headers, "referer", "https://podcaster.xiaoyuzhoufm.com/");
    assert_header(&captured.headers, "user-agent", PODCASTER_UA);
    assert!(
        json_eq(
            &captured.body,
            r#"{"areaCode":"+86","mobilePhoneNumber":"13800000000","captcha":{"scene":"web","verifyParam":"verify-param-123"}}"#
        ),
        "body 不一致：{}",
        captured.body
    );
}

#[test]
fn 发送验证码_缺省区号默认86_golden() {
    let captured = mock_once(
        r#"{"code":200,"msg":"OK","data":null}"#,
        &[],
        |base| {
            let client = ApiClient::with_bases(base.clone(), base);
            client
                .send_code(&SendCodeRequest {
                    mobile_phone_number: "13800000000".into(),
                    area_code: None,
                    captcha: CaptchaPayload::web("vp-default"),
                })
                .unwrap();
        },
    );
    assert!(
        json_eq(
            &captured.body,
            r#"{"areaCode":"+86","mobilePhoneNumber":"13800000000","captcha":{"scene":"web","verifyParam":"vp-default"}}"#
        ),
        "缺省区号应回落到 +86：{}",
        captured.body
    );
}

#[test]
fn 发送验证码_无加号区号自动补全_golden() {
    let captured = mock_once(
        r#"{"code":200,"msg":"OK","data":null}"#,
        &[],
        |base| {
            let client = ApiClient::with_bases(base.clone(), base);
            client
                .send_code(&SendCodeRequest {
                    mobile_phone_number: "13800000000".into(),
                    area_code: Some("86".into()),
                    captcha: CaptchaPayload::web("vp-norm"),
                })
                .unwrap();
        },
    );
    assert!(
        json_eq(
            &captured.body,
            r#"{"areaCode":"+86","mobilePhoneNumber":"13800000000","captcha":{"scene":"web","verifyParam":"vp-norm"}}"#
        ),
        "无 + 前缀的区号应自动补全为 +86：{}",
        captured.body
    );
}

// ---------------------------------------------------------------------------
// login
// ---------------------------------------------------------------------------

#[test]
fn 登录_golden_从响应头提取token() {
    let captured = mock_once(
        r#"{"code":200,"msg":"OK","data":{"user":{"uid":"u1","nickname":"昵称","type":"user"}}}"#,
        &[
            ("x-jike-access-token", "AT-1"),
            ("x-jike-refresh-token", "RT-1"),
        ],
        |base| {
            let client = ApiClient::with_bases(base.clone(), base);
            let result = client
                .login(&LoginRequest {
                    mobile_phone_number: "13800000000".into(),
                    verify_code: "1234".into(),
                    area_code: Some("+86".into()),
                })
                .unwrap();
            assert_eq!(result.access_token.as_deref(), Some("AT-1"));
            assert_eq!(result.refresh_token.as_deref(), Some("RT-1"));
            assert_eq!(result.data.as_ref().unwrap().uid, "u1");
            assert_eq!(result.data.as_ref().unwrap().nickname.as_deref(), Some("昵称"));
        },
    );

    assert_eq!(captured.method, "POST");
    assert_eq!(captured.path, "/v1/auth/login-with-sms");
    assert_header(&captured.headers, "accept", "application/json, text/plain, */*");
    assert_header(&captured.headers, "content-type", "application/json;charset=UTF-8");
    assert_header(&captured.headers, "origin", "https://podcaster.xiaoyuzhoufm.com");
    assert_header(&captured.headers, "referer", "https://podcaster.xiaoyuzhoufm.com/");
    assert_header(&captured.headers, "user-agent", PODCASTER_UA);
    assert!(
        json_eq(
            &captured.body,
            r#"{"areaCode":"+86","mobilePhoneNumber":"13800000000","verifyCode":"1234"}"#
        ),
        "body 不一致：{}",
        captured.body
    );
}

// ---------------------------------------------------------------------------
// refresh_token
// ---------------------------------------------------------------------------

#[test]
fn 刷新token_golden() {
    let captured = mock_once(
        r#"{}"#,
        &[
            ("x-jike-access-token", "AT-2"),
            ("x-jike-refresh-token", "RT-2"),
        ],
        |base| {
            let client = ApiClient::with_bases(base.clone(), base);
            let data = client
                .refresh_token(&RefreshTokenRequest {
                    x_jike_access_token: "AT-old".into(),
                    x_jike_refresh_token: "RT-old".into(),
                })
                .unwrap();
            assert_eq!(data.access_token.as_deref(), Some("AT-2"));
            assert_eq!(data.refresh_token.as_deref(), Some("RT-2"));
        },
    );

    assert_eq!(captured.method, "POST");
    assert_eq!(captured.path, "/app_auth_tokens.refresh");
    assert_header(&captured.headers, "Host", "api.xiaoyuzhoufm.com");
    assert_header(
        &captured.headers,
        "User-Agent",
        "Xiaoyuzhou/2.57.1 (build:1576; iOS 17.4.1)",
    );
    assert_header(&captured.headers, "x-jike-access-token", "AT-old");
    assert_header(&captured.headers, "x-jike-refresh-token", "RT-old");
    assert_header(
        &captured.headers,
        "Content-Type",
        "application/x-www-form-urlencoded; charset=utf-8",
    );
    assert_header(&captured.headers, "App-Version", "2.57.1");
    // Go 侧 `json.Marshal(nil)` 得到字面量 `null`。
    assert_eq!(captured.body.trim(), "null");
}

#[test]
fn 刷新token_响应头缺省时回落到响应体_golden() {
    let captured = mock_once(
        r#"{"x-jike-access-token":"AT-body","x-jike-refresh-token":"RT-body"}"#,
        &[],
        |base| {
            let client = ApiClient::with_bases(base.clone(), base);
            let data = client
                .refresh_token(&RefreshTokenRequest {
                    x_jike_access_token: "AT-old".into(),
                    x_jike_refresh_token: "RT-old".into(),
                })
                .unwrap();
            assert_eq!(data.access_token.as_deref(), Some("AT-body"));
            assert_eq!(data.refresh_token.as_deref(), Some("RT-body"));
        },
    );
    assert_eq!(captured.path, "/app_auth_tokens.refresh");
}

// ---------------------------------------------------------------------------
// 业务接口 golden 对照
// ---------------------------------------------------------------------------

const AB_ENABLE: &str = r#"{"old_user_discovery_feed":"enable"}"#;
const LANG_HKCN: &str = "zh-Hant-HK;q=1.0, zh-Hans-CN;q=0.9";
const LANG_CNTW: &str = "zh-Hans-CN;q=1.0, zh-Hant-TW;q=0.9";
const LANG_CNHK: &str = "zh-Hans-CN;q=1.0, zh-Hant-HK;q=0.9";
const LANG_TWCN: &str = "zh-Hant-CN;q=1.0, zh-Hans-HK;q=0.9";
const FIXED_DID: &str = "81ADBFD6-6921-482B-9AB9-A29E7CC7BB55";
const H5_UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_4_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Xiaoyuzhou/2.57.1 (build:1576; iOS 17.4.1) xyzTheme/defaultLight";
const H5_LANG: &str = "zh-CN,zh-Hans;q=0.9";
const UA_1858: &str = "Xiaoyuzhou/2.57.1 (build:1858; iOS 17.4.1)";
const CT: &str = "application/json";

/// 通用对照：断言 method/path、必在头、必缺头、请求体与 Go handler 一致。
fn check<F>(method: &str, path: &str, present: &[(&str, &str)], absent: &[&str], body: Option<&str>, run: F)
where
    F: FnOnce(String),
{
    let captured = mock_once(r#"{"code":200,"msg":"OK"}"#, &[], run);
    assert_eq!(captured.method, method, "{path} 方法不一致");
    assert_eq!(captured.path, path, "路径不一致");
    for (k, v) in present {
        assert_header(&captured.headers, k, v);
    }
    for k in absent {
        assert!(
            header_value(&captured.headers, k).is_none(),
            "{path} 不应包含头 {k}"
        );
    }
    if let Some(b) = body {
        assert!(json_eq(&captured.body, b), "{path} body 不一致：{}", captured.body);
    }
}

// ---------------------------------------------------------------------------
// 搜索
// ---------------------------------------------------------------------------

#[test]
fn g_搜索() {
    check(
        "POST",
        "/v1/search/create",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &[],
        Some(r#"{"limit":"20","sourcePageName":"4","type":"PODCAST","currentPageName":"4","keyword":"小宇宙"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.search(&SearchRequest {
                keyword: "小宇宙".into(),
                kind: "PODCAST".into(),
                pid: None,
                load_more_key: None,
            });
        },
    );
}

#[test]
fn g_搜索预设() {
    check(
        "GET",
        "/v1/search/get-preset",
        &[("Accept-Language", LANG_HKCN)],
        &["abtest-info", "Content-Type"],
        None,
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.search_preset();
        },
    );
}

// ---------------------------------------------------------------------------
// 单集
// ---------------------------------------------------------------------------

#[test]
fn g_单集列表() {
    check(
        "POST",
        "/v1/episode/list",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &[],
        Some(r#"{"limit":"20","pid":"p1","order":"desc"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.episode_list(&EpisodeListRequest {
                pid: "p1".into(),
                order: "desc".into(),
                load_more_key: None,
            });
        },
    );
}

#[test]
fn g_最受欢迎单集() {
    check(
        "POST",
        "/v1/episode/list-by-filter",
        &[("abtest-info", "{}"), ("Accept-Language", LANG_CNTW), ("Content-Type", CT)],
        &[],
        Some(r#"{"pid":"p1","label":"POPULAR"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.episode_list_by_filter("p1");
        },
    );
}

#[test]
fn g_单集详情() {
    check(
        "GET",
        "/v1/episode/get?eid=e1",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN)],
        &["Content-Type"],
        None,
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.episode_detail("e1");
        },
    );
}

#[test]
fn g_收听人数() {
    check(
        "GET",
        "/v1/live-stats/episode/get?eid=e1",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN)],
        &["Content-Type"],
        None,
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.episode_live_count("e1");
        },
    );
}

#[test]
fn g_播放进度() {
    check(
        "POST",
        "/v1/playback-progress/list",
        &[("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &["abtest-info"],
        Some(r#"{"eids":["e1","e2"]}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.episode_play_progress(&["e1".to_string(), "e2".to_string()]);
        },
    );
}

#[test]
fn g_更新播放进度() {
    check(
        "POST",
        "/v1/playback-progress/update",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &[],
        Some(r#"{"data":[{"pid":"p1","eid":"e1","progress":30,"playedAt":"2024-01-01T00:00:00+08:00"}]}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.episode_play_progress_update(&[PlaybackProgressItem {
                pid: "p1".into(),
                eid: "e1".into(),
                progress: 30,
                played_at: "2024-01-01T00:00:00+08:00".into(),
            }]);
        },
    );
}

#[test]
fn g_上报播放状态() {
    check(
        "POST",
        "/v1/live-stats/report",
        &[("abtest-info", "{}"), ("Accept-Language", LANG_CNTW), ("Content-Type", CT)],
        &[],
        Some(r#"{"playStats":{"action":"PLAY","eid":"e1","pid":"p1"}}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.live_stats_report(&LiveStatsReportRequest { eid: "e1".into(), pid: "p1".into() });
        },
    );
}

#[test]
fn g_精彩时间点() {
    check(
        "POST",
        "/v1/clap/list",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("X-Online-Host", "api.xiaoyuzhoufm.com")],
        &[],
        Some(r#"{"eid":"e1","duration":7284}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.episode_clap("e1", 7284);
        },
    );
}

#[test]
fn g_标记精彩() {
    check(
        "POST",
        "/v1/clap/create",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("X-Online-Host", "api.xiaoyuzhoufm.com")],
        &[],
        Some(r#"{"eid":"e1","timestamp":30,"duration":5,"currentPageName":7,"sourcePageName":8}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.episode_clap_create("e1", 30, 5);
        },
    );
}

#[test]
fn g_收听历史记录() {
    check(
        "POST",
        "/v1/episode-played/list",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("X-Online-Host", "api.xiaoyuzhoufm.com")],
        &[],
        Some(r#"{"uid":"u1"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.played_list("u1");
        },
    );
}

// ---------------------------------------------------------------------------
// 节目
// ---------------------------------------------------------------------------

#[test]
fn g_节目详情() {
    check(
        "GET",
        "/v1/podcast/get?pid=p1",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN)],
        &["Content-Type"],
        None,
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.podcast_detail("p1");
        },
    );
}

#[test]
fn g_节目主体信息() {
    check(
        "GET",
        "/v1/podcast/get-info?pid=p1",
        &[("User-Agent", H5_UA), ("Accept-Language", H5_LANG)],
        &["Content-Type", "abtest-info"],
        None,
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.podcast_get_info("p1");
        },
    );
}

#[test]
fn g_节目荣誉墙() {
    check(
        "POST",
        "/v1/podcast-honor/list",
        &[("Accept-Language", H5_LANG), ("Content-Type", CT)],
        &["abtest-info"],
        Some(r#"{"pid":"p1"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.podcast_honor_list("p1");
        },
    );
}

#[test]
fn g_相关节目() {
    check(
        "POST",
        "/v1/related-podcast/list",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &[],
        Some(r#"{"pid":"p1","position":"BOTTOM"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.podcast_related("p1");
        },
    );
}

#[test]
fn g_节目公告() {
    check(
        "GET",
        "/v1/podcast-bulletin/get-by-pid?pid=p1",
        &[("abtest-info", "{}"), ("Accept-Language", LANG_CNTW), ("User-Agent", UA_1858)],
        &["Content-Type"],
        None,
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.podcast_bulletin("p1");
        },
    );
}

#[test]
fn g_用户播客() {
    check(
        "POST",
        "/v1/podcaster/owned-podcasts",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &[],
        Some(r#"{"uid":"u1"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.owned_podcasts("u1");
        },
    );
}

// ---------------------------------------------------------------------------
// 订阅
// ---------------------------------------------------------------------------

#[test]
fn g_订阅列表() {
    check(
        "POST",
        "/v1/subscription/list",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &[],
        Some(r#"{"limit":"20","sortOrder":"desc","sortBy":"subscribedAt"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.subscription(&SubscriptionRequest { uid: None, load_more_key: None });
        },
    );
}

#[test]
fn g_更新订阅() {
    check(
        "POST",
        "/v1/subscription/update",
        &[("abtest-info", "{}"), ("Accept-Language", LANG_CNTW), ("Content-Type", CT)],
        &[],
        Some(r#"{"pid":"p1","mode":"ON"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.subscription_update(&SubscriptionUpdateRequest { pid: "p1".into(), status: "ON".into() });
        },
    );
}

#[test]
fn g_星标订阅() {
    check(
        "POST",
        "/v1/subscription-star/list",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &[],
        Some(r#"{}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.subscription_star();
        },
    );
}

#[test]
fn g_未加星标() {
    check(
        "POST",
        "/v1/subscription/list-non-starred",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &[],
        Some(r#"{}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.subscription_non_starred();
        },
    );
}

#[test]
fn g_更新星标() {
    check(
        "POST",
        "/v1/subscription-star/update",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &[],
        Some(r#"{"pid":"p1","withStar":true}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.subscription_star_update("p1", true);
        },
    );
}

// ---------------------------------------------------------------------------
// 评论
// ---------------------------------------------------------------------------

#[test]
fn g_一级评论() {
    check(
        "POST",
        "/v1/comment/list-primary",
        &[("Content-Type", CT)],
        &["abtest-info", "Accept-Language"],
        Some(r#"{"order":"HOT","owner":{"id":"e1","type":"EPISODE"}}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.comment_primary("e1", "EPISODE", "HOT", None);
        },
    );
}

#[test]
fn g_一级评论_带游标() {
    check(
        "POST",
        "/v1/comment/list-primary",
        &[("Content-Type", CT)],
        &["abtest-info", "Accept-Language"],
        Some(r#"{"order":"HOT","owner":{"id":"e1","type":"EPISODE"},"loadMoreKey":{"hotSortScore":1.5,"id":"lm1","direction":"NEXT","section":"1"}}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.comment_primary(
                "e1",
                "EPISODE",
                "HOT",
                Some(CommentPrimaryLoadMoreKey {
                    id: "lm1".into(),
                    direction: "NEXT".into(),
                    hot_sort_score: 1.5,
                    section: "1".into(),
                }),
            );
        },
    );
}

#[test]
fn g_回复评论() {
    check(
        "POST",
        "/v1/comment/list-thread",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("X-Online-Host", "api.xiaoyuzhoufm.com"), ("Content-Type", CT)],
        &[],
        Some(r#"{"order":"HOT","primaryCommentId":"c1"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.comment_thread("HOT", "c1");
        },
    );
}

#[test]
fn g_收藏评论() {
    check(
        "POST",
        "/v1/comment/collect/create",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("X-Online-Host", "api.xiaoyuzhoufm.com"), ("Content-Type", CT)],
        &[],
        Some(r#"{"commentId":"c1"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.comment_collect_create("c1");
        },
    );
}

#[test]
fn g_取消收藏评论() {
    check(
        "POST",
        "/v1/comment/collect/remove",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("X-Online-Host", "api.xiaoyuzhoufm.com"), ("Content-Type", CT)],
        &[],
        Some(r#"{"commentId":"c1"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.comment_collect_remove("c1");
        },
    );
}

#[test]
fn g_收藏评论列表() {
    check(
        "POST",
        "/v1/comment/collect/list",
        &[("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &["abtest-info"],
        Some(r#"{}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.comment_collect_list(None);
        },
    );
}

#[test]
fn g_点赞评论() {
    check(
        "POST",
        "/v1/like/update",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("X-Online-Host", "api.xiaoyuzhoufm.com"), ("Content-Type", CT)],
        &[],
        Some(r#"{"liked":true,"target":{"id":"c1","type":"COMMENT"}}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.comment_like_update(true, "c1", "COMMENT");
        },
    );
}

#[test]
fn g_创建评论() {
    check(
        "POST",
        "/v1/comment/create",
        &[("abtest-info", "{}"), ("Accept-Language", LANG_CNHK), ("X-Online_Host", "api.xiaoyuzhoufm.com"), ("Content-Type", CT)],
        &[],
        Some(r#"{"text":"hi","owner":{"id":"e1","type":"EPISODE"}}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.comment_create("hi", "e1", "EPISODE", None);
        },
    );
}

#[test]
fn g_创建评论_回复() {
    check(
        "POST",
        "/v1/comment/create",
        &[("abtest-info", "{}"), ("Accept-Language", LANG_CNHK), ("X-Online_Host", "api.xiaoyuzhoufm.com"), ("Content-Type", CT)],
        &[],
        Some(r#"{"text":"hi","owner":{"id":"e1","type":"EPISODE"},"replyToCommentId":"c1"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.comment_create("hi", "e1", "EPISODE", Some("c1"));
        },
    );
}

#[test]
fn g_删除评论() {
    check(
        "POST",
        "/v1/comment/remove",
        &[("abtest-info", "{}"), ("Accept-Language", LANG_CNHK), ("X-Online-Host", "api.xiaoyuzhoufm.com"), ("Content-Type", CT)],
        &[],
        Some(r#"{"commentId":"c1"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.comment_remove("c1");
        },
    );
}

// ---------------------------------------------------------------------------
// 首页发现 / 榜单
// ---------------------------------------------------------------------------

#[test]
fn g_首页发现() {
    check(
        "POST",
        "/v1/discovery-feed/list",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &[],
        Some(r#"{"returnAll":"false"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.discovery(None);
        },
    );
}

#[test]
fn g_榜单() {
    check(
        "GET",
        "/v1/top-list/get?category=HOT_EPISODES_IN_24_HOURS",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN)],
        &["Content-Type"],
        None,
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.top_list("HOT");
        },
    );
}

#[test]
fn g_编辑精选历史() {
    check(
        "POST",
        "/v1/editor-pick/list-history",
        &[("abtest-info", "{}"), ("Accept-Language", LANG_TWCN), ("X-Online-Host", "api.xiaoyuzhoufm.com"), ("Content-Type", CT)],
        &[],
        Some(r#"{}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.editor_pick_list_history(None);
        },
    );
}

#[test]
fn g_新节目广场() {
    check(
        "POST",
        "/v1/pilot-discovery/list",
        &[("abtest-info", "{}"), ("Accept-Language", LANG_TWCN), ("Content-Type", CT)],
        &[],
        Some(r#"{}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.pilot_discovery_list();
        },
    );
}

#[test]
fn g_刷新推荐() {
    check(
        "POST",
        "/v1/discovery-collection/refresh-episode-recommend",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &[],
        Some(r#"{}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.refresh_episode_recommend();
        },
    );
}

// ---------------------------------------------------------------------------
// 用户 / 个人资料 / 贴纸
// ---------------------------------------------------------------------------

#[test]
fn g_我的资料() {
    check(
        "GET",
        "/v1/profile/get",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN)],
        &["Content-Type"],
        None,
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.profile("u1");
        },
    );
}

#[test]
fn g_他人资料() {
    check(
        "GET",
        "/v1/profile/get?uid=u1",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("x-jike-device-id", FIXED_DID)],
        &["Content-Type"],
        None,
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.get_profile("u1");
        },
    );
}

#[test]
fn g_用户统计() {
    check(
        "GET",
        "/v1/user-stats/get?uid=u1",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("x-jike-device-id", FIXED_DID)],
        &["Content-Type"],
        None,
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.user_stats("u1");
        },
    );
}

#[test]
fn g_贴纸列表() {
    check(
        "POST",
        "/v1/sticker/list",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &[],
        Some(r#"{"uid":"u1"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.sticker("u1");
        },
    );
}

#[test]
fn g_贴纸墙() {
    check(
        "POST",
        "/v1/sticker/get-board",
        &[("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &["abtest-info"],
        Some(r#"{"uid":"u1"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.sticker_board("u1");
        },
    );
}

// ---------------------------------------------------------------------------
// 收藏 / 关注 / 历史 / 设置
// ---------------------------------------------------------------------------

#[test]
fn g_收藏单集() {
    check(
        "POST",
        "/v1/favorite/update",
        &[("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &["abtest-info"],
        Some(r#"{"eid":"e1","favorited":true,"sourcePageName":8,"currentPageName":9}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.favorite_episode_update("e1", true);
        },
    );
}

#[test]
fn g_取消收藏单集() {
    check(
        "POST",
        "/v1/favorite/update",
        &[("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &["abtest-info"],
        Some(r#"{"eid":"e1","favorited":false,"sourcePageName":8,"currentPageName":9}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.favorite_episode_update("e1", false);
        },
    );
}

#[test]
fn g_收藏列表() {
    check(
        "POST",
        "/v1/favorite/list",
        &[("abtest-info", "{}"), ("Accept-Language", LANG_CNTW), ("Content-Type", CT)],
        &[],
        Some(r#"{}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.favorite_episode_list();
        },
    );
}

#[test]
fn g_收听历史列表() {
    check(
        "POST",
        "/v1/episode-played/list-history",
        &[("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &["abtest-info"],
        Some(r#"{}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.episode_played_history_list(None);
        },
    );
}

#[test]
fn g_更新收听历史() {
    check(
        "POST",
        "/v1/episode-played/create",
        &[("abtest-info", "{}"), ("Accept-Language", LANG_CNTW), ("X-Online-Host", "api.xiaoyuzhoufm.com"), ("Content-Type", CT)],
        &[],
        Some(r#"{"eid":"e1"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.episode_played_history_list_update("e1");
        },
    );
}

#[test]
fn g_订阅更新列表() {
    check(
        "POST",
        "/v1/inbox/list",
        &[("Accept-Language", LANG_CNTW), ("Content-Type", CT)],
        &["abtest-info"],
        Some(r#"{"limit":"20"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.inbox_list(None);
        },
    );
}

#[test]
fn g_订阅更新列表_带游标() {
    check(
        "POST",
        "/v1/inbox/list",
        &[("Accept-Language", LANG_CNTW), ("Content-Type", CT)],
        &["abtest-info"],
        Some(r#"{"limit":"20","loadMoreKey":{"pubDate":"2024-01-01","id":"lm1"}}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.inbox_list(Some(InboxLoadMoreKey {
                pub_date: "2024-01-01".into(),
                id: "lm1".into(),
            }));
        },
    );
}

#[test]
fn g_未读消息数() {
    check(
        "GET",
        "/v1/unread-count/get",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("X-Online-Host", "api.xiaoyuzhoufm.com")],
        &["Content-Type"],
        None,
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.unread_count();
        },
    );
}

#[test]
fn g_关注列表() {
    check(
        "POST",
        "/v1/user-relation/list-following",
        &[("Content-Type", CT)],
        &["abtest-info", "Accept-Language"],
        Some(r#"{"uid":"u1"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.following_list("u1");
        },
    );
}

#[test]
fn g_粉丝列表() {
    check(
        "POST",
        "/v1/user-relation/list-follower",
        &[("Content-Type", CT)],
        &["abtest-info", "Accept-Language"],
        Some(r#"{"uid":"u1"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.follower_list("u1");
        },
    );
}

#[test]
fn g_关注用户() {
    check(
        "POST",
        "/v1/user-relation/update",
        &[("abtest-info", "{}"), ("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &[],
        Some(r#"{"uid":"u1","relation":"FOLLOW"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.relation_update("u1", "FOLLOW");
        },
    );
}

#[test]
fn g_偏好设置() {
    check(
        "GET",
        "/v1/user-preference/get",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN)],
        &["Content-Type"],
        None,
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.user_preference_get();
        },
    );
}

#[test]
fn g_更新偏好() {
    check(
        "POST",
        "/v1/user-preference/update",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN)],
        &["Content-Type"],
        Some(r#"{"playbackRate":true}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.user_preference_update("playbackRate", true);
        },
    );
}

#[test]
fn g_付费单集音频() {
    check(
        "GET",
        "/v1/private-media/get?dubbing=false&eid=e1",
        &[("app-permissions", "100000")],
        &["abtest-info", "Accept-Language", "Content-Type"],
        None,
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.private_media_get("e1");
        },
    );
}

// ---------------------------------------------------------------------------
// 分类
// ---------------------------------------------------------------------------

#[test]
fn g_全部分类() {
    check(
        "POST",
        "/v1/category/list-all",
        &[("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &["abtest-info"],
        Some(r#"{}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.category_list();
        },
    );
}

#[test]
fn g_分类标签() {
    check(
        "POST",
        "/v1/category/podcast/list-tabs",
        &[("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &["abtest-info"],
        Some(r#"{"categoryId":"c1"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.category_list_tab("c1");
        },
    );
}

#[test]
fn g_分类节目列表() {
    check(
        "POST",
        "/v1/category/podcast/list-by-tab",
        &[("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &["abtest-info"],
        Some(r#"{"categoryId":"c1","omitSubscribed":false,"tab":"t1"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.category_podcast_list("c1", false, "t1", None);
        },
    );
}

// ---------------------------------------------------------------------------
// 里程（收听数据）
// ---------------------------------------------------------------------------

#[test]
fn g_收听数据概览() {
    check(
        "GET",
        "/v1/mileage/get",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &[],
        None,
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.mileage_get();
        },
    );
}

#[test]
fn g_收听排行() {
    check(
        "POST",
        "/v1/mileage/list",
        &[("abtest-info", "{}"), ("Accept-Language", LANG_CNTW), ("X-Online-Host", "api.xiaoyuzhoufm.com"), ("Content-Type", CT)],
        &[],
        Some(r#"{"rank":"TOTAL"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.mileage_list(true);
        },
    );
}

#[test]
fn g_更新收听数据() {
    check(
        "POST",
        "/v1/mileage/update",
        &[("abtest-info", "{}"), ("Accept-Language", LANG_CNTW), ("X-Online-Host", "api.xiaoyuzhoufm.com"), ("Content-Type", CT)],
        &[],
        Some(r#"{"tracking":[{"eid":"e1","pid":"p1","startPlayingTimestamp":1.0,"endPlayingTimestamp":2.0,"isSpeaker":false,"isOffline":false,"isTrial":false,"withSpeed":1.0}]}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.mileage_update(&[MileageTracking {
                eid: "e1".into(),
                pid: "p1".into(),
                start_playing_timestamp: 1.0,
                end_playing_timestamp: 2.0,
                is_speaker: false,
                is_offline: false,
                is_trial: false,
                with_speed: 1.0,
            }]);
        },
    );
}

// ---------------------------------------------------------------------------
// 喜欢（pick）
// ---------------------------------------------------------------------------

#[test]
fn g_喜欢片段() {
    check(
        "POST",
        "/v1/pick/list-recent",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &[],
        Some(r#"{"uid":"u1"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.pick_list_recent("u1");
        },
    );
}

#[test]
fn g_喜欢全部() {
    check(
        "POST",
        "/v1/pick/list-history",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("Content-Type", CT)],
        &[],
        Some(r#"{"uid":"u1"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.pick_list_history("u1", None);
        },
    );
}

// ---------------------------------------------------------------------------
// 黑名单
// ---------------------------------------------------------------------------

#[test]
fn g_黑名单列表() {
    check(
        "POST",
        "/v1/blocked-user/list",
        &[("Content-Type", CT)],
        &["abtest-info", "Accept-Language"],
        Some(r#"{}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.blocked_user_lists();
        },
    );
}

#[test]
fn g_加入黑名单() {
    check(
        "POST",
        "/v1/blocked-user/create",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("X-Online-Host", "api.xiaoyuzhoufm.com"), ("Content-Type", CT)],
        &[],
        Some(r#"{"uid":"u1"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.blocked_user_create("u1");
        },
    );
}

#[test]
fn g_移出黑名单() {
    check(
        "POST",
        "/v1/blocked-user/remove",
        &[("abtest-info", AB_ENABLE), ("Accept-Language", LANG_HKCN), ("X-Online-Host", "api.xiaoyuzhoufm.com"), ("Content-Type", CT)],
        &[],
        Some(r#"{"uid":"u1"}"#),
        |base| {
            let c = ApiClient::with_bases(base.clone(), base);
            let _ = c.blocked_user_remove("u1");
        },
    );
}
