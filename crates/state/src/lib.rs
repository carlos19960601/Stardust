//! 应用全局状态层（对齐 sonora 的 `state` crate）。
//!
//! 本模块负责登录会话（access/refresh token 与用户信息）的生命周期：
//! - [`Session`]：可序列化的会话数据；
//! - `load_session` / `save_session` / `clear_session`：会话的磁盘持久化；
//! - [`AuthManager`]：登录、退出、token 刷新，以及刷新后的自动持久化；
//! - [`spawn_refresh_loop`]：后台线程周期刷新 token（每 10 分钟）。
//!
//! 设计要点：
//! - 所有网络调用走 `stardust-api` 的强类型客户端（ureq 同步 HTTP）；
//! - 会话持久化到磁盘后，应用重启时 [`AuthManager::new`] 会自动恢复；
//! - token 通过后台线程周期刷新，实现「token 失效时自动刷新」。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use stardust_api::{ApiClient, DEFAULT_BASE_URL, LoginRequest, RefreshTokenRequest, SendCodeRequest, User};

/// 登录会话：access/refresh token 与当前用户信息。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    pub user: Option<User>,
}

impl Session {
    /// 会话是否可用（至少持有 access token）。
    pub fn is_valid(&self) -> bool {
        !self.access_token.is_empty()
    }
}

/// 会话持久化文件名（相对配置目录）。
pub const SESSION_FILE: &str = "session.json";

/// 默认配置目录：`~/Library/Application Support/Stardust`（macOS）。
pub fn default_config_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("Stardust")
}

/// 从磁盘加载会话；文件不存在或损坏时返回 `None`。
pub fn load_session(dir: &Path) -> Option<Session> {
    let path = dir.join(SESSION_FILE);
    let text = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str::<Session>(&text) {
        Ok(session) if session.is_valid() => Some(session),
        _ => None,
    }
}

/// 保存会话到磁盘（自动创建目录）。
pub fn save_session(dir: &Path, session: &Session) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(SESSION_FILE);
    let text = serde_json::to_string_pretty(session)?;
    std::fs::write(&path, text)?;
    Ok(())
}

/// 删除持久化的会话文件。
pub fn clear_session(dir: &Path) -> anyhow::Result<()> {
    let path = dir.join(SESSION_FILE);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// 认证管理器：封装 API 客户端与登录会话，负责登录、持久化与刷新。
///
/// 内部可变（token 会刷新），在 UI 与后台刷新线程之间共享时请使用
/// `Arc<Mutex<AuthManager>>`。
pub struct AuthManager {
    client: ApiClient,
    session: Option<Session>,
    config_dir: PathBuf,
}

impl AuthManager {
    /// 创建管理器并尝试恢复持久化会话。
    pub fn new(config_dir: PathBuf) -> Self {
        let session = load_session(&config_dir);
        let mut client = ApiClient::new(DEFAULT_BASE_URL);
        if let Some(s) = &session {
            client.set_access_token(s.access_token.clone());
        }
        log::info!(
            "认证管理器初始化：{}",
            if session.is_some() { "已恢复会话" } else { "未登录" }
        );
        Self {
            client,
            session,
            config_dir,
        }
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn client(&self) -> &ApiClient {
        &self.client
    }

    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    pub fn is_logged_in(&self) -> bool {
        self.session.as_ref().map(Session::is_valid).unwrap_or(false)
    }

    pub fn user(&self) -> Option<&User> {
        self.session.as_ref().and_then(|s| s.user.as_ref())
    }

    /// 发送短信验证码。
    pub fn send_code(&self, phone: &str, area_code: &str) -> anyhow::Result<()> {
        let req = SendCodeRequest {
            mobile_phone_number: phone.to_string(),
            area_code: non_empty(area_code),
        };
        self.client.send_code(&req)?;
        Ok(())
    }

    /// 验证码登录，成功后更新并持久化会话。
    pub fn login(&mut self, phone: &str, code: &str, area_code: &str) -> anyhow::Result<()> {
        let req = LoginRequest {
            mobile_phone_number: phone.to_string(),
            verify_code: code.to_string(),
            area_code: non_empty(area_code),
        };
        let result = self.client.login(&req)?;

        let access = result
            .access_token
            .filter(|t| !t.is_empty())
            .ok_or_else(|| anyhow::anyhow!("登录响应缺少 access token"))?;
        let refresh = result.refresh_token.unwrap_or_default();
        let session = Session {
            access_token: access.clone(),
            refresh_token: refresh,
            user: result.data,
        };

        self.client.set_access_token(access);
        save_session(&self.config_dir, &session)?;
        self.session = Some(session);
        log::info!("登录成功");
        Ok(())
    }

    /// 刷新 token 并持久化。
    pub fn refresh(&mut self) -> anyhow::Result<()> {
        let Some(session) = self.session.clone() else {
            anyhow::bail!("尚未登录，无法刷新 token");
        };

        let req = RefreshTokenRequest {
            x_jike_access_token: session.access_token.clone(),
            x_jike_refresh_token: session.refresh_token.clone(),
        };
        let data = self.client.refresh_token(&req)?;

        let access = data
            .access_token
            .filter(|t| !t.is_empty())
            .ok_or_else(|| anyhow::anyhow!("刷新响应缺少 access token"))?;
        let refresh = data
            .refresh_token
            .filter(|t| !t.is_empty())
            .unwrap_or(session.refresh_token.clone());

        let updated = Session {
            access_token: access.clone(),
            refresh_token: refresh,
            user: session.user,
        };

        self.client.set_access_token(access);
        save_session(&self.config_dir, &updated)?;
        self.session = Some(updated);
        log::info!("token 已刷新");
        Ok(())
    }

    /// 退出登录：清空内存会话并删除持久化文件。
    pub fn logout(&mut self) -> anyhow::Result<()> {
        self.session = None;
        self.client.clear_access_token();
        clear_session(&self.config_dir)?;
        log::info!("已退出登录");
        Ok(())
    }
}

/// 非空字符串转 `Option<String>`。
fn non_empty(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// 启动后台 token 刷新线程：每 10 分钟刷新一次（对齐 horizon 前端的轮询间隔）。
pub fn spawn_refresh_loop(auth: Arc<Mutex<AuthManager>>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(10 * 60));
        match auth.lock() {
            Ok(mut manager) => {
                if manager.is_logged_in() {
                    if let Err(e) = manager.refresh() {
                        log::warn!("自动刷新 token 失败：{e:#}");
                    }
                }
            }
            Err(_) => break,
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// 每个测试独立的临时目录，避免并行测试相互影响。
    fn temp_dir(name: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "stardust-state-{name}-{}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_session() -> Session {
        Session {
            access_token: "access-token-123".into(),
            refresh_token: "refresh-token-456".into(),
            user: Some(User {
                uid: "u1".into(),
                nickname: Some("测试用户".into()),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn 会话序列化往返() {
        let session = sample_session();
        let json = serde_json::to_string(&session).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(back.access_token, "access-token-123");
        assert_eq!(back.refresh_token, "refresh-token-456");
        assert_eq!(back.user.unwrap().nickname.as_deref(), Some("测试用户"));
    }

    #[test]
    fn 会话有效性判断() {
        assert!(sample_session().is_valid());
        assert!(!Session::default().is_valid());
    }

    #[test]
    fn 会话持久化往返() {
        let dir = temp_dir("session");
        save_session(&dir, &sample_session()).unwrap();
        let loaded = load_session(&dir).unwrap();
        assert_eq!(loaded.access_token, "access-token-123");
        assert_eq!(loaded.user.unwrap().uid, "u1");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 清理会话文件() {
        let dir = temp_dir("clear");
        save_session(&dir, &sample_session()).unwrap();
        clear_session(&dir).unwrap();
        assert!(load_session(&dir).is_none());
        // 再次清理不存在的文件也应成功
        clear_session(&dir).unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 加载损坏的会话返回none() {
        let dir = temp_dir("corrupt");
        std::fs::write(dir.join(SESSION_FILE), "not-json").unwrap();
        assert!(load_session(&dir).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 认证管理器启动时恢复会话() {
        let dir = temp_dir("restore");
        save_session(&dir, &sample_session()).unwrap();
        let manager = AuthManager::new(dir.clone());
        assert!(manager.is_logged_in());
        assert_eq!(manager.client().access_token(), Some("access-token-123"));
        assert_eq!(manager.user().unwrap().uid, "u1");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 认证管理器未登录时无会话() {
        let dir = temp_dir("fresh");
        let manager = AuthManager::new(dir.clone());
        assert!(!manager.is_logged_in());
        assert!(manager.user().is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 未登录时刷新报错() {
        let dir = temp_dir("refresh");
        let mut manager = AuthManager::new(dir.clone());
        assert!(manager.refresh().is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 非空字符串转换() {
        assert_eq!(non_empty(""), None);
        assert_eq!(non_empty("86"), Some("86".to_string()));
    }

    // ---------------------------------------------------------------------
    // 鉴权流程 mock 测试（直连上游客户端 + 本地一次性假服务器）
    // ---------------------------------------------------------------------
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};

    /// 读空一次 HTTP 请求（解析 Content-Length 以确定 body 结束），避免与 ureq 死锁。
    fn drain_request(stream: &mut TcpStream) {
        let mut buf: Vec<u8> = Vec::new();
        let mut tmp = [0u8; 8192];
        let header_end;
        loop {
            let n = stream.read(&mut tmp).unwrap();
            assert!(n > 0, "连接被对端提前关闭");
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                header_end = pos + 4;
                break;
            }
        }
        let header_text = String::from_utf8_lossy(&buf[..header_end]);
        let content_length = header_text
            .lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.eq_ignore_ascii_case("content-length")
                    .then(|| v.trim().parse::<usize>().unwrap_or(0))
            })
            .unwrap_or(0);
        let mut body = buf[header_end..].to_vec();
        while body.len() < content_length {
            let mut tmp = [0u8; 8192];
            let n = stream.read(&mut tmp).unwrap();
            if n == 0 {
                break;
            }
            body.extend_from_slice(&tmp[..n]);
        }
    }

    fn write_response(stream: &mut TcpStream, body: &str, headers: &[(&str, &str)]) {
        let mut resp = String::from("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n");
        for (k, v) in headers {
            resp.push_str(&format!("{k}: {v}\r\n"));
        }
        resp.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body));
        stream.write_all(resp.as_bytes()).unwrap();
        stream.flush().ok();
    }

    #[test]
    fn 登录流程_mock_保存会话() {
        let dir = temp_dir("login-mock");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");

        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            drain_request(&mut stream);
            write_response(
                &mut stream,
                r#"{"code":200,"msg":"OK","data":{"user":{"uid":"u1","nickname":"昵称","type":"user"}}}"#,
                &[("x-jike-access-token", "AT-L"), ("x-jike-refresh-token", "RT-L")],
            );
        });

        let mut mgr = AuthManager {
            client: ApiClient::with_bases(base.clone(), base),
            session: None,
            config_dir: dir.clone(),
        };
        mgr.login("13800000000", "1234", "+86").unwrap();
        handle.join().unwrap();

        assert!(mgr.is_logged_in());
        assert_eq!(mgr.session().unwrap().access_token, "AT-L");
        assert_eq!(mgr.session().unwrap().refresh_token, "RT-L");
        assert_eq!(mgr.user().unwrap().uid, "u1");
        assert!(dir.join(SESSION_FILE).exists(), "登录后应持久化会话文件");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 刷新流程_mock_更新会话() {
        let dir = temp_dir("refresh-mock");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");

        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            drain_request(&mut stream);
            write_response(
                &mut stream,
                r#"{}"#,
                &[("x-jike-access-token", "AT-NEW"), ("x-jike-refresh-token", "RT-NEW")],
            );
        });

        let mut mgr = AuthManager {
            client: ApiClient::with_bases(base.clone(), base),
            session: Some(Session {
                access_token: "AT-old".into(),
                refresh_token: "RT-old".into(),
                user: None,
            }),
            config_dir: dir.clone(),
        };
        mgr.refresh().unwrap();
        handle.join().unwrap();

        assert_eq!(mgr.session().unwrap().access_token, "AT-NEW");
        assert_eq!(mgr.session().unwrap().refresh_token, "RT-NEW");
        assert_eq!(mgr.client().access_token(), Some("AT-NEW"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
