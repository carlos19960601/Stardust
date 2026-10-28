//! 本地一次性人机验证回调服务（对齐小宇宙上游阿里云验证码 2.0 契约）。
//!
//! 小宇宙短信发送接口（`/v1/auth/send-code`）要求请求体携带阿里云验证码 2.0
//! 前端 SDK 生成的 `captcha: { scene: "web", verifyParam: "..." }`。
//!
//! 本模块在 `127.0.0.1:0`（仅本机回环）启动一次性轻量 HTTP 服务，打开系统默认
//! 浏览器加载安全验证页：
//! - 常规环境下触发阿里云无痕验证（`TRACELESS`），无需手动操作即可自动回传
//!   `verifyParam` 并由本地服务调用上游发送短信；
//! - 若触发风控滑块或上游返回 `code = 1037`（验证码校验未通过），页面内直接展示
//!   滑块供用户完成验证重试；
//! - 支持一次性随机 `token` 校验、超时回收与客户端/网页双向取消。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use stardust_api::ApiError;

/// 上游表示「阿里云人机验证未通过，需重新完成验证」的业务错误码。
pub const CAPTCHA_RETRY_CODE: i32 = 1037;

/// 安全验证默认总超时时长（90 秒）。
pub const DEFAULT_CAPTCHA_TIMEOUT: Duration = Duration::from_secs(90);

/// 单次 HTTP 连接读写超时（防止预连接空闲句柄阻塞监听循环）。
const STREAM_IO_TIMEOUT: Duration = Duration::from_secs(5);

/// 允许的最大请求体字节数（64 KiB，阿里云 `verifyParam` 约 2~4 KiB）。
const MAX_BODY_BYTES: usize = 64 * 1024;

/// 小宇宙统一身份认证生产环境使用的阿里云验证码配置。
const ALIYUN_PREFIX: &str = "kn7vz1";
const ALIYUN_SCENE_ID: &str = "80c00qbb";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VerifyCallbackBody {
    verify_param: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VerifyCallbackResponse {
    ok: bool,
    retry_captcha: bool,
    message: String,
}

/// 手机号脱敏展示（如 `19961780601` -> `199****0601`），并对特殊字符做 HTML 转义。
pub fn mask_phone(phone: &str) -> String {
    let trimmed = phone.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    let raw = if chars.len() >= 7 {
        let prefix: String = chars[..3].iter().collect();
        let suffix: String = chars[chars.len() - 4..].iter().collect();
        format!("{prefix}****{suffix}")
    } else {
        trimmed.to_string()
    };
    escape_html(&raw)
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// 使用系统默认浏览器打开指定 URL。
pub fn open_system_browser(url: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    let status = Command::new("open").arg(url).status()?;

    #[cfg(target_os = "linux")]
    let status = Command::new("xdg-open").arg(url).status()?;

    #[cfg(target_os = "windows")]
    let status = Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .status()?;

    if !status.success() {
        anyhow::bail!("无法拉起系统浏览器（退出码：{status}）");
    }
    Ok(())
}

/// 启动本地一次性安全验证服务，待浏览器完成阿里云人机验证后调用 `submit_verify` 发送短信。
pub fn run_captcha_send_flow<O, S>(
    phone: &str,
    cancel: &AtomicBool,
    timeout: Duration,
    open_browser: O,
    mut submit_verify: S,
) -> anyhow::Result<()>
where
    O: FnOnce(&str) -> anyhow::Result<()>,
    S: FnMut(&str) -> Result<(), ApiError>,
{
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| anyhow::anyhow!("绑定本地安全验证端口失败：{e}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| anyhow::anyhow!("配置本地验证监听套接字失败：{e}"))?;

    let port = listener
        .local_addr()
        .map_err(|e| anyhow::anyhow!("读取本地验证端口失败：{e}"))?
        .port();
    let token = uuid::Uuid::new_v4().simple().to_string();
    let verify_url = format!("http://127.0.0.1:{port}/?token={token}");
    let page_html = build_verify_page_html(&mask_phone(phone), &token);

    open_browser(&verify_url)?;

    let deadline = Instant::now() + timeout;
    loop {
        if cancel.load(Ordering::Relaxed) {
            anyhow::bail!("已取消安全验证");
        }
        if Instant::now() >= deadline {
            anyhow::bail!("安全验证超时，请重新点击发送验证码");
        }

        match listener.accept() {
            Ok((mut stream, _addr)) => {
                // macOS/BSD 下 accept 出的套接字可能继承非阻塞标志，显式恢复为带超时的阻塞模式。
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(STREAM_IO_TIMEOUT));
                let _ = stream.set_write_timeout(Some(STREAM_IO_TIMEOUT));

                let Some(req) = read_http_request(&mut stream) else {
                    // 忽略浏览器 TCP 预连接或半开连接，继续等待真实请求。
                    continue;
                };

                let (route, query) = split_path_query(&req.target);

                if req.method == "GET" && route == "/favicon.ico" {
                    write_http_response(&mut stream, "204 No Content", "text/plain", b"");
                    continue;
                }

                // 除 /favicon.ico 外的所有路由均须校验一次性会话 token，防止跨站伪造调用。
                if query_param(query, "token") != Some(token.as_str()) {
                    write_http_response(
                        &mut stream,
                        "403 Forbidden",
                        "text/plain; charset=utf-8",
                        "无效或已过期的安全验证会话".as_bytes(),
                    );
                    continue;
                }

                match (req.method.as_str(), route) {
                    ("GET", "/") => {
                        write_http_response(
                            &mut stream,
                            "200 OK",
                            "text/html; charset=utf-8",
                            page_html.as_bytes(),
                        );
                    }
                    ("POST", "/cancel") => {
                        write_http_response(
                            &mut stream,
                            "200 OK",
                            "application/json; charset=utf-8",
                            br#"{"ok":true}"#,
                        );
                        anyhow::bail!("已取消安全验证");
                    }
                    ("POST", "/verify") => {
                        let parsed = serde_json::from_slice::<VerifyCallbackBody>(&req.body);
                        let verify_param = match parsed {
                            Ok(b) if !b.verify_param.trim().is_empty() => b.verify_param,
                            _ => {
                                let resp = VerifyCallbackResponse {
                                    ok: false,
                                    retry_captcha: true,
                                    message: "未收到有效的人机验证参数".to_string(),
                                };
                                write_json_response(&mut stream, "400 Bad Request", &resp);
                                continue;
                            }
                        };

                        match submit_verify(&verify_param) {
                            Ok(()) => {
                                let resp = VerifyCallbackResponse {
                                    ok: true,
                                    retry_captcha: false,
                                    message: "验证码已发送".to_string(),
                                };
                                write_json_response(&mut stream, "200 OK", &resp);
                                return Ok(());
                            }
                            Err(ApiError::Api { code, msg }) if code == CAPTCHA_RETRY_CODE => {
                                let message = if msg.is_empty() {
                                    "人机验证未通过，请重新拖动滑块".to_string()
                                } else {
                                    msg
                                };
                                let resp = VerifyCallbackResponse {
                                    ok: false,
                                    retry_captcha: true,
                                    message,
                                };
                                write_json_response(&mut stream, "200 OK", &resp);
                                // 保持监听，等待用户在页面内完成新一轮滑块验证。
                            }
                            Err(err) => {
                                let resp = VerifyCallbackResponse {
                                    ok: false,
                                    retry_captcha: false,
                                    message: err.to_string(),
                                };
                                write_json_response(&mut stream, "200 OK", &resp);
                                return Err(err.into());
                            }
                        }
                    }
                    _ => {
                        write_http_response(
                            &mut stream,
                            "404 Not Found",
                            "text/plain; charset=utf-8",
                            b"Not Found",
                        );
                    }
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                anyhow::bail!("本地安全验证服务监听异常：{e}");
            }
        }
    }
}

struct ParsedHttpRequest {
    method: String,
    target: String,
    body: Vec<u8>,
}

/// 读取一次完整的 HTTP/1.1 请求；连接为空、超时或格式异常时返回 `None`。
fn read_http_request(stream: &mut TcpStream) -> Option<ParsedHttpRequest> {
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let mut tmp = [0u8; 4096];
    let header_end = loop {
        match stream.read(&mut tmp) {
            Ok(0) => return None,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break pos + 4;
                }
                if buf.len() > MAX_BODY_BYTES {
                    return None;
                }
            }
            Err(_) => return None,
        }
    };

    let header_text = std::str::from_utf8(&buf[..header_end]).ok()?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_uppercase();
    let target = parts.next()?.to_string();

    let mut content_length = 0usize;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                content_length = v.trim().parse::<usize>().ok()?;
            }
        }
    }
    if content_length > MAX_BODY_BYTES {
        return None;
    }

    let mut body = buf[header_end..].to_vec();
    while body.len() < content_length {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => body.extend_from_slice(&tmp[..n]),
            Err(_) => return None,
        }
    }
    if body.len() < content_length {
        return None;
    }
    body.truncate(content_length);

    Some(ParsedHttpRequest {
        method,
        target,
        body,
    })
}

fn split_path_query(target: &str) -> (&str, &str) {
    match target.split_once('?') {
        Some((path, query)) => (path, query),
        None => (target, ""),
    }
}

fn query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then_some(v)
    })
}

fn write_json_response<T: Serialize>(stream: &mut TcpStream, status: &str, payload: &T) {
    let bytes = serde_json::to_vec(payload).unwrap_or_else(|_| br#"{"ok":false}"#.to_vec());
    write_http_response(stream, status, "application/json; charset=utf-8", &bytes);
}

fn write_http_response(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) {
    let header = format!(
        "HTTP/1.1 {status}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

/// 生成与 Stardust 暗色主题一致的本地安全验证页。
fn build_verify_page_html(masked_phone: &str, token: &str) -> String {
    format!(
        r##"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>Stardust · 小宇宙安全验证</title>
  <style>
    * {{ box-sizing: border-box; margin: 0; padding: 0; }}
    body {{
      min-height: 100vh;
      display: flex;
      align-items: center;
      justify-content: center;
      background: #141414;
      color: #F0F0F0;
      font-family: -apple-system, BlinkMacSystemFont, "PingFang SC", "Microsoft YaHei", sans-serif;
      padding: 24px;
    }}
    .card {{
      width: 100%;
      max-width: 400px;
      background: #1E1E1E;
      border: 1px solid #303030;
      border-radius: 12px;
      padding: 32px 28px;
      display: flex;
      flex-direction: column;
      align-items: center;
      gap: 16px;
      box-shadow: 0 16px 40px rgba(0, 0, 0, 0.45);
    }}
    .title {{
      font-size: 18px;
      font-weight: 600;
      color: #E6B84C;
    }}
    .subtitle {{
      font-size: 13px;
      color: #999999;
      text-align: center;
      line-height: 1.6;
    }}
    .status {{
      font-size: 14px;
      color: #D8D8D8;
      text-align: center;
      line-height: 1.6;
      min-height: 22px;
    }}
    .status.ok {{ color: #6CBF84; font-weight: 500; }}
    .status.err {{ color: #E06C5B; }}
    #captcha-wrap {{
      min-height: 54px;
      width: 100%;
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      gap: 12px;
    }}
    #get-captcha-btn {{
      padding: 9px 20px;
      border-radius: 8px;
      border: none;
      background: #E6B84C;
      color: #141414;
      font-size: 14px;
      font-weight: 600;
      cursor: pointer;
    }}
    #get-captcha-btn:disabled {{
      opacity: 0.55;
      cursor: default;
    }}
    .cancel-btn {{
      margin-top: 4px;
      background: transparent;
      border: none;
      color: #777777;
      font-size: 12px;
      cursor: pointer;
      text-decoration: underline;
    }}
    .cancel-btn:hover {{ color: #BBBBBB; }}
  </style>
</head>
<body>
  <div class="card">
    <div class="title">小宇宙 · 安全验证</div>
    <div class="subtitle">正在为手机号 {masked_phone} 获取登录验证码</div>
    <div id="status" class="status">正在加载安全验证组件…</div>
    <div id="captcha-wrap">
      <div id="captcha-element"></div>
      <button id="get-captcha-btn" type="button" disabled>正在准备验证…</button>
    </div>
    <button id="cancel-btn" class="cancel-btn" type="button">取消并返回客户端</button>
  </div>

  <script>
    const TOKEN = "{token}";
    const statusEl = document.getElementById("status");
    const triggerBtn = document.getElementById("get-captcha-btn");
    const cancelBtn = document.getElementById("cancel-btn");
    const captchaEl = document.getElementById("captcha-element");
    let finished = false;

    function setStatus(text, cls) {{
      statusEl.textContent = text;
      statusEl.className = cls ? ("status " + cls) : "status";
    }}

    // 监测滑块节点是否展现，动态更新引导文案。
    const observer = new MutationObserver(() => {{
      if (finished) return;
      const sliding = captchaEl.querySelector("#aliyunCaptcha-sliding-body");
      if (sliding && getComputedStyle(sliding).display === "block") {{
        setStatus("请拖动下方滑块完成人机验证", "");
        triggerBtn.style.display = "none";
      }}
    }});
    observer.observe(captchaEl, {{ childList: true, subtree: true, attributes: true }});

    cancelBtn.addEventListener("click", async () => {{
      if (finished) {{
        window.close();
        return;
      }}
      finished = true;
      try {{
        await fetch("/cancel?token=" + TOKEN, {{ method: "POST" }});
      }} catch (_) {{}}
      document.getElementById("captcha-wrap").style.display = "none";
      cancelBtn.style.display = "none";
      setStatus("已取消安全验证，请返回 Stardust 客户端。", "err");
      setTimeout(() => window.close(), 300);
    }});

    window.AliyunCaptchaConfig = {{ region: "cn", prefix: "{ALIYUN_PREFIX}" }};
  </script>
  <script
    src="https://o.alicdn.com/captcha-frontend/aliyunCaptcha/AliyunCaptcha.js"
    onerror="document.getElementById('status').textContent='安全验证脚本加载失败，请检查网络后重试';document.getElementById('status').className='status err';"
  ></script>
  <script>
    window.addEventListener("load", () => {{
      if (typeof window.initAliyunCaptcha !== "function") {{
        setStatus("安全验证组件初始化失败，请检查网络连接", "err");
        return;
      }}
      window.initAliyunCaptcha({{
        SceneId: "{ALIYUN_SCENE_ID}",
        mode: "embed",
        immediate: true,
        element: "#captcha-element",
        button: "#get-captcha-btn",
        slideStyle: {{ width: 320, height: 51 }},
        language: "cn",
        getInstance: () => {{
          if (finished) return;
          triggerBtn.disabled = false;
          triggerBtn.textContent = "点击完成安全验证";
          setStatus("正在进行安全验证…", "");
          // 实例就绪后自动触发一次：若通过无痕验证可直接发送短信，无需用户额外点击。
          setTimeout(() => {{
            if (!finished) triggerBtn.click();
          }}, 120);
        }},
        captchaVerifyCallback: async (verifyParam) => {{
          setStatus("验证通过，正在请求发送短信验证码…", "");
          try {{
            const resp = await fetch("/verify?token=" + TOKEN, {{
              method: "POST",
              headers: {{ "Content-Type": "application/json" }},
              body: JSON.stringify({{ verifyParam }}),
            }});
            const data = await resp.json();
            if (data.ok) {{
              finished = true;
              document.getElementById("captcha-wrap").style.display = "none";
              cancelBtn.textContent = "关闭此页面";
              setStatus("✓ 短信验证码已发送，请返回 Stardust 客户端输入验证码", "ok");
              setTimeout(() => window.close(), 600);
              return {{ captchaResult: true, bizResult: true }};
            }}
            if (data.retryCaptcha) {{
              triggerBtn.style.display = "inline-block";
              setStatus(data.message || "验证未通过，请重新验证", "err");
              return {{ captchaResult: false, bizResult: true }};
            }}
            finished = true;
            document.getElementById("captcha-wrap").style.display = "none";
            cancelBtn.textContent = "关闭此页面";
            setStatus("发送失败：" + (data.message || "未知错误") + "（请返回客户端查看）", "err");
            return {{ captchaResult: true, bizResult: false }};
          }} catch (e) {{
            setStatus("与 Stardust 本地服务通信失败：" + e.message, "err");
            return {{ captchaResult: false, bizResult: false }};
          }}
        }},
        onBizResultCallback: () => {{}},
      }});
    }});
  </script>
</body>
</html>"##
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn local_agent() -> ureq::Agent {
        ureq::AgentBuilder::new().try_proxy_from_env(false).build()
    }

    #[test]
    fn 手机号脱敏_标准11位与短号() {
        assert_eq!(mask_phone("19961780601"), "199****0601");
        assert_eq!(mask_phone(" 13800138000 "), "138****8000");
        assert_eq!(mask_phone("12345"), "12345");
        assert_eq!(mask_phone("<tag>"), "&lt;tag&gt;");
    }

    #[test]
    fn 本地验证流_无痕验证一次通过() {
        let cancel = AtomicBool::new(false);
        let mut received = String::new();

        let res = run_captcha_send_flow(
            "19961780601",
            &cancel,
            Duration::from_secs(5),
            |url| {
                let url = url.to_string();
                std::thread::spawn(move || {
                    // 1. 模拟浏览器预连接空套接字（不应中断服务）
                    let host_port = url
                        .trim_start_matches("http://")
                        .split('/')
                        .next()
                        .unwrap();
                    let _preconnect = TcpStream::connect(host_port).unwrap();
                    drop(_preconnect);

                    // 2. 加载验证页 HTML
                    let agent = local_agent();
                    let html = agent.get(&url).call().unwrap().into_string().unwrap();
                    assert!(html.contains("199****0601"));
                    assert!(html.contains(ALIYUN_SCENE_ID));

                    // 3. 回传 verifyParam
                    let token = url.split("token=").nth(1).unwrap();
                    let verify_url = format!("http://{host_port}/verify?token={token}");
                    let resp: serde_json::Value = agent
                        .post(&verify_url)
                        .send_json(serde_json::json!({ "verifyParam": "vp-ok-123" }))
                        .unwrap()
                        .into_json()
                        .unwrap();
                    assert_eq!(resp["ok"], true);
                });
                Ok(())
            },
            |vp| {
                received = vp.to_string();
                Ok(())
            },
        );

        assert!(res.is_ok());
        assert_eq!(received, "vp-ok-123");
    }

    #[test]
    fn 本地验证流_1037重试后成功() {
        let cancel = AtomicBool::new(false);
        let mut attempts = 0usize;

        let res = run_captcha_send_flow(
            "13800000000",
            &cancel,
            Duration::from_secs(5),
            |url| {
                let url = url.to_string();
                std::thread::spawn(move || {
                    let host_port = url
                        .trim_start_matches("http://")
                        .split('/')
                        .next()
                        .unwrap();
                    let token = url.split("token=").nth(1).unwrap();

                    // 伪造错误 token 应返回 403 且不影响后续正常验证
                    let agent = local_agent();
                    let bad = agent
                        .post(&format!("http://{host_port}/verify?token=wrong"))
                        .send_json(serde_json::json!({ "verifyParam": "x" }));
                    assert!(matches!(bad, Err(ureq::Error::Status(403, _))));

                    let verify_url = format!("http://{host_port}/verify?token={token}");
                    // 第一次返回 1037 要求重试
                    let r1: serde_json::Value = agent
                        .post(&verify_url)
                        .send_json(serde_json::json!({ "verifyParam": "vp-first" }))
                        .unwrap()
                        .into_json()
                        .unwrap();
                    assert_eq!(r1["ok"], false);
                    assert_eq!(r1["retryCaptcha"], true);

                    // 第二次重试通过
                    let r2: serde_json::Value = agent
                        .post(&verify_url)
                        .send_json(serde_json::json!({ "verifyParam": "vp-second" }))
                        .unwrap()
                        .into_json()
                        .unwrap();
                    assert_eq!(r2["ok"], true);
                });
                Ok(())
            },
            |vp| {
                attempts += 1;
                if vp == "vp-first" {
                    Err(ApiError::Api {
                        code: CAPTCHA_RETRY_CODE,
                        msg: "滑块校验失败".into(),
                    })
                } else {
                    Ok(())
                }
            },
        );

        assert!(res.is_ok());
        assert_eq!(attempts, 2);
    }

    #[test]
    fn 本地验证流_业务错误立即透传退出() {
        let cancel = AtomicBool::new(false);

        let err = run_captcha_send_flow(
            "13800000000",
            &cancel,
            Duration::from_secs(5),
            |url| {
                let url = url.to_string();
                std::thread::spawn(move || {
                    let host_port = url
                        .trim_start_matches("http://")
                        .split('/')
                        .next()
                        .unwrap();
                    let token = url.split("token=").nth(1).unwrap();
                    let verify_url = format!("http://{host_port}/verify?token={token}");
                    let r: serde_json::Value = local_agent()
                        .post(&verify_url)
                        .send_json(serde_json::json!({ "verifyParam": "vp-any" }))
                        .unwrap()
                        .into_json()
                        .unwrap();
                    assert_eq!(r["ok"], false);
                    assert_eq!(r["retryCaptcha"], false);
                });
                Ok(())
            },
            |_vp| {
                Err(ApiError::Api {
                    code: 1013,
                    msg: "该手机号未注册小宇宙".into(),
                })
            },
        )
        .unwrap_err();

        assert!(
            format!("{err:#}").contains("该手机号未注册小宇宙"),
            "实际错误：{err:#}"
        );
    }

    #[test]
    fn 本地验证流_支持客户端原子标记取消() {
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_bg = cancel.clone();

        let err = run_captcha_send_flow(
            "13800000000",
            &cancel,
            Duration::from_secs(5),
            |_url| {
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(80));
                    cancel_bg.store(true, Ordering::SeqCst);
                });
                Ok(())
            },
            |_vp| Ok(()),
        )
        .unwrap_err();

        assert!(format!("{err:#}").contains("已取消安全验证"));
    }

    /// 端到端冒烟验证（需联网与 `agent-browser`，默认忽略）：
    /// 启动本地验证服务 -> 无头浏览器加载阿里云验证码并自动回传真实 `verifyParam` ->
    /// 请求小宇宙真实上游 `/v1/auth/send-code`（使用未注册测试号 `13800000000`，不会发真实短信），
    /// 断言通过 `body/captcha` 参数结构校验并返回业务码 `1013`（该手机号未注册小宇宙）。
    #[test]
    #[ignore]
    fn 真实浏览器与上游schema联调_未注册号返回1013() {
        let cancel = AtomicBool::new(false);
        let client = stardust_api::ApiClient::new(stardust_api::DEFAULT_BASE_URL);
        let mut got_real_param_len = 0usize;

        let res = run_captcha_send_flow(
            "13800000000",
            &cancel,
            Duration::from_secs(20),
            |url| {
                let url = url.to_string();
                std::thread::spawn(move || {
                    let _ = Command::new("agent-browser").args(["open", &url]).status();
                });
                Ok(())
            },
            |vp| {
                got_real_param_len = vp.len();
                client
                    .send_code(&stardust_api::SendCodeRequest {
                        mobile_phone_number: "13800000000".into(),
                        area_code: Some("86".into()),
                        captcha: stardust_api::CaptchaPayload::web(vp),
                    })
                    .map(|_| ())
            },
        );

        let _ = Command::new("agent-browser").arg("close").status();
        assert!(got_real_param_len > 100, "应获取到真实阿里云 verifyParam");
        let err = res.expect_err("未注册号码应返回业务错误 1013");
        let msg = format!("{err:#}");
        println!("真实联调结果（verifyParam 长度={got_real_param_len}）：{msg}");
        assert!(
            msg.contains("1013") && msg.contains("该手机号未注册小宇宙"),
            "实际返回：{msg}"
        );
    }
}
