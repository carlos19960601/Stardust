//! Stardust —— 小宇宙（Xiaoyuzhou FM）的 GPUI 桌面客户端入口。
//!
//! 参考实现：
//! - 代码组织与 GPUI 实现方式：[sonora](https://github.com/sonorahq/sonora)
//! - UI 功能对标：[horizon](https://github.com/ultrazg/horizon)
//! - 内嵌 API 服务：[xyz](https://github.com/ultrazg/xyz)

mod service;

use gpui::{
    App, Application, Bounds, Window, WindowBounds, WindowOptions, div, px, rgb, size,
    prelude::*,
};

/// 应用根视图。骨架阶段先渲染一个占位窗口，后续替换为 Router 驱动的页面结构。
struct RootView;

impl gpui::Render for RootView {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .bg(rgb(0x1a1a1a))
            .text_color(rgb(0xffffff))
            .text_lg()
            .child("Stardust")
            .child("小宇宙 GPUI 桌面客户端")
    }
}

fn main() {
    // 日志初始化。
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // 先安装信号处理器，确保异常退出时也能回收内嵌服务。
    service::install_signal_handlers();

    // 启动内嵌的 xyz（小宇宙 API）服务。
    // 该守卫在 main 返回时 Drop，正常退出路径同样会回收子进程。
    let _api_service = match service::ApiService::start() {
        Ok(svc) => svc,
        Err(err) => {
            eprintln!("内嵌 xyz 服务启动失败: {err:#}");
            std::process::exit(1);
        }
    };

    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(960.), px(640.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_window, cx| cx.new(|_cx| RootView),
        )
        .expect("无法创建主窗口");

        cx.activate(true);
    });
}
