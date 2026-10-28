//! Stardust —— 小宇宙（Xiaoyuzhou FM）的 GPUI 桌面客户端入口。
//!
//! 参考实现：
//! - 代码组织与 GPUI 实现方式：[sonora](https://github.com/sonorahq/sonora)
//! - UI 功能对标：[horizon](https://github.com/ultrazg/horizon)
//!
//! API 访问由 `stardust-api` 直连小宇宙上游（`https://api.xiaoyuzhoufm.com`），
//! 不再内嵌 Go 编写的 xyz 代理服务。

use std::sync::{Arc, Mutex};

use gpui::{px, size, App, Application, Bounds, WindowBounds, WindowOptions};
use gpui::prelude::*;
use stardust_state::{AuthManager, default_config_dir, spawn_refresh_loop};
use stardust_views::LoginView;

fn main() {
    // 日志初始化。
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // 认证管理器：启动时自动恢复持久化会话，供登录页与后续页面共享。
    let auth = Arc::new(Mutex::new(AuthManager::new(default_config_dir())));
    // 后台线程周期刷新 token（每 10 分钟），实现「token 失效时自动刷新」。
    spawn_refresh_loop(auth.clone());

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(960.), px(640.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_window, cx| cx.new(|cx| LoginView::new(auth.clone(), cx)),
        )
        .expect("无法创建主窗口");

        cx.activate(true);
    });
}
