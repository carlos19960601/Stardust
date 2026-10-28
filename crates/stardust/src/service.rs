//! 内嵌 xyz（小宇宙 API）服务的子进程生命周期管理。
//!
//! 思路对齐 horizon：应用启动时在 `localhost:23020` 拉起 xyz 服务，退出时回收。
//! - xyz 二进制由 `build.rs` 编译并内嵌进可执行文件，运行时提取到临时目录；
//! - 通过 RAII（`Drop`）保证正常退出时回收子进程；
//! - 安装 SIGINT/SIGTERM 信号处理器，保证 `cargo run` 下 Ctrl+C 或 `kill` 也不残留孤儿进程。

use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};

/// xyz 服务监听的默认端口（与 xyz 的 `-p 23020` 一致）。
const SERVICE_PORT: u16 = 23020;

/// 内嵌的 xyz 二进制（由 build.rs 编译到 OUT_DIR）。
const XYZ_BINARY: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/xyz"));

/// 当前由本应用持有的 xyz 子进程 PID（-1 表示未持有），供信号处理器 kill 使用。
static CHILD_PID: AtomicI32 = AtomicI32::new(-1);

/// xyz 子进程的 RAII 守卫：Drop 时回收子进程并清理临时二进制。
pub struct ApiService {
    child: Option<Child>,
    binary_path: Option<PathBuf>,
}

impl ApiService {
    /// 启动内嵌的 xyz 服务，并阻塞等待其就绪。
    ///
    /// 若端口已被外部进程占用，则复用之、不重复启动，也不接管其生命周期。
    pub fn start() -> anyhow::Result<ApiService> {
        if port_in_use(SERVICE_PORT) {
            log::warn!("端口 {SERVICE_PORT} 已被占用，复用外部 xyz 服务");
            return Ok(ApiService {
                child: None,
                binary_path: None,
            });
        }

        let binary_path = extract_binary()?;
        let child = Command::new(&binary_path)
            .arg("-p")
            .arg(SERVICE_PORT.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("无法启动内嵌 xyz 服务: {}", binary_path.display()))?;

        CHILD_PID.store(child.id() as i32, Ordering::SeqCst);

        wait_ready(Duration::from_secs(15))?;
        log::info!("内嵌 xyz 服务已就绪: http://localhost:{SERVICE_PORT}");

        Ok(ApiService {
            child: Some(child),
            binary_path: Some(binary_path),
        })
    }
}

impl Drop for ApiService {
    fn drop(&mut self) {
        CHILD_PID.store(-1, Ordering::SeqCst);

        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(path) = self.binary_path.take() {
            let _ = std::fs::remove_file(path);
        }
        log::info!("内嵌 xyz 服务已回收");
    }
}

/// 安装 SIGINT/SIGTERM 信号处理器。
///
/// 在独立线程中等待信号，收到后先 kill 子进程再退出进程，避免孤儿进程。
pub fn install_signal_handlers() {
    use signal_hook::consts::{SIGINT, SIGTERM};
    use signal_hook::iterator::Signals;

    match Signals::new([SIGINT, SIGTERM]) {
        Ok(mut signals) => {
            std::thread::spawn(move || {
                for sig in signals.forever() {
                    let pid = CHILD_PID.swap(-1, Ordering::SeqCst);
                    if pid > 0 {
                        unsafe { libc::kill(pid, libc::SIGKILL) };
                    }
                    std::process::exit(128 + sig);
                }
            });
        }
        Err(err) => {
            log::warn!("无法安装信号处理器，异常退出时可能残留 xyz 子进程: {err}");
        }
    }
}

/// 将内嵌的 xyz 二进制写到临时目录并赋予可执行权限。
fn extract_binary() -> anyhow::Result<PathBuf> {
    let dir = std::env::temp_dir().join("stardust");
    std::fs::create_dir_all(&dir)?;

    let path = dir.join("xyz");
    std::fs::write(&path, XYZ_BINARY)?;

    let mut perms = std::fs::metadata(&path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms)?;

    Ok(path)
}

fn port_in_use(port: u16) -> bool {
    TcpStream::connect(("127.0.0.1", port)).is_ok()
}

/// 轮询等待服务端口可连接，超时则报错。
fn wait_ready(timeout: Duration) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if port_in_use(SERVICE_PORT) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("内嵌 xyz 服务在 {} 秒内未就绪", timeout.as_secs());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
