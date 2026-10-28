//! 页面与视图层（对齐 sonora 的 `views` crate）。
//!
//! 本阶段（task-auth）实现登录页：
//! - [`TextField`]：基于 GPUI 原语自建的单行文本框（焦点 + 键盘输入 + 粘贴 + 光标）；
//! - [`LoginView`]：手机号 + 验证码登录、本地浏览器人机安全验证发送验证码、
//!   token 会话管理、风控免责声明；登录成功后展示当前用户与退出入口。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::prelude::*;
use gpui::{
    div, px, rgb, Context, Div, Entity, FocusHandle, KeyDownEvent, Stateful, Window,
};
use stardust_state::{AuthManager, send_code_via_browser};

/// 当前时间（Unix 秒）。
fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// TextField
// ---------------------------------------------------------------------------

/// 单行文本框：支持字符输入、剪贴板粘贴、退格删除与占位提示。
///
/// gpui 0.2.2 未内置 TextInput 组件，这里用 `div().id(...).focusable()` +
/// `track_focus` + 键盘事件实现轻量输入框，满足登录表单的输入需求。
pub struct TextField {
    id: &'static str,
    text: String,
    placeholder: String,
    digits_only: bool,
    focus_handle: FocusHandle,
}

impl TextField {
    pub fn new(
        cx: &mut Context<Self>,
        id: &'static str,
        placeholder: impl Into<String>,
        digits_only: bool,
    ) -> Self {
        Self {
            id,
            text: String::new(),
            placeholder: placeholder.into(),
            digits_only,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    fn append_filtered(&mut self, input: &str) {
        for c in input.chars() {
            if c.is_control() {
                continue;
            }
            if self.digits_only && !c.is_ascii_digit() {
                continue;
            }
            self.text.push(c);
        }
    }

    fn handle_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let m = &event.keystroke.modifiers;
        // 支持 Cmd/Ctrl + V 粘贴与 Cmd/Ctrl + Backspace 清空，其余组合快捷键忽略。
        if m.platform || m.control {
            match event.keystroke.key.as_str() {
                "v" => {
                    if let Some(clip) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        self.append_filtered(&clip);
                    }
                }
                "backspace" => {
                    self.text.clear();
                }
                _ => {}
            }
            return;
        }

        match event.keystroke.key.as_str() {
            "backspace" => {
                self.text.pop();
            }
            "enter" | "tab" | "escape" | "arrowleft" | "arrowright" => {}
            _ => {
                // 优先取实际键入字符（key_char），否则退回物理键字符。
                let ch = event
                    .keystroke
                    .key_char
                    .clone()
                    .unwrap_or_else(|| event.keystroke.key.clone());
                if ch.chars().count() != 1 {
                    return;
                }
                self.append_filtered(&ch);
            }
        }
    }
}

impl gpui::Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus_handle.is_focused(window);
        // 聚焦时在末尾追加光标标记。
        let mut display = self.text.clone();
        if focused {
            display.push('▏');
        }
        let is_empty = self.text.is_empty();
        let placeholder = self.placeholder.clone();

        div()
            .id(self.id)
            .focusable()
            .track_focus(&self.focus_handle)
            .on_click(cx.listener(|this, _e, window, _cx| {
                this.focus_handle.focus(window);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                this.handle_key(event, cx);
                cx.notify();
            }))
            .flex()
            .items_center()
            .px_3()
            .py_2()
            .rounded_md()
            .border_1()
            .border_color(rgb(if focused { 0xE6B84C } else { 0x3A3A3A }))
            .bg(rgb(0x242424))
            .text_color(if is_empty { rgb(0x777777) } else { rgb(0xF0F0F0) })
            .text_base()
            .child(if is_empty { placeholder } else { display })
    }
}

// ---------------------------------------------------------------------------
// LoginView
// ---------------------------------------------------------------------------

/// 登录流程阶段。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// 展示登录表单。
    Form,
    /// 已登录，展示当前用户。
    LoggedIn,
}

/// 提示消息语义类别（区分正常进度、成功与错误配色）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum MessageTone {
    Info,
    Success,
    Error,
}

/// 风控与版权免责声明（展示在登录按钮下方）。
const DISCLAIMER: &str = "风险提示：本客户端为非官方第三方实现，登录行为受小宇宙平台风控策略约束，\
请勿频繁获取验证码；账号与验证码安全由用户自行负责。本项目仅供学习交流，请遵守平台服务条款，\
不对因使用本软件造成的账号限制或损失承担责任。";

/// 发送验证码的冷却时长（秒）。
const SEND_CODE_COOLDOWN: u64 = 60;

/// 默认国际电话区号。
const DEFAULT_AREA_CODE: &str = "+86";

/// 登录页：手机号 + 验证码登录。
pub struct LoginView {
    auth: Arc<Mutex<AuthManager>>,
    phone: Entity<TextField>,
    code: Entity<TextField>,
    phase: Phase,
    message: String,
    message_tone: MessageTone,
    last_send_epoch: u64,
    verifying_cancel: Option<Arc<AtomicBool>>,
    logging_in: bool,
}

impl LoginView {
    pub fn new(auth: Arc<Mutex<AuthManager>>, cx: &mut Context<Self>) -> Self {
        let phone = cx.new(|cx| TextField::new(cx, "phone-field", "请输入手机号", true));
        let code = cx.new(|cx| TextField::new(cx, "code-field", "请输入验证码", true));
        let already_logged_in = auth
            .lock()
            .map(|a| a.is_logged_in())
            .unwrap_or(false);
        Self {
            auth,
            phone,
            code,
            phase: if already_logged_in {
                Phase::LoggedIn
            } else {
                Phase::Form
            },
            message: String::new(),
            message_tone: MessageTone::Info,
            last_send_epoch: 0,
            verifying_cancel: None,
            logging_in: false,
        }
    }

    fn set_message(&mut self, tone: MessageTone, text: impl Into<String>) {
        self.message_tone = tone;
        self.message = text.into();
    }

    /// 当前登录用户昵称（用于登录成功后的展示）。
    fn nickname(&self) -> String {
        self.auth
            .lock()
            .ok()
            .and_then(|a| {
                a.user()
                    .map(|u| u.nickname.clone().unwrap_or_else(|| u.uid.clone()))
            })
            .unwrap_or_else(|| "用户".to_string())
    }

    /// 启动每秒刷新的重发倒计时协程，使按钮秒数实时递减。
    fn start_cooldown_ticker(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(1))
                    .await;
                let keep_running = this
                    .update(cx, |view, cx| {
                        cx.notify();
                        epoch_secs() < view.last_send_epoch + SEND_CODE_COOLDOWN
                    })
                    .unwrap_or(false);
                if !keep_running {
                    break;
                }
            }
        })
        .detach();
    }

    /// 发送验证码：拉起本地一次性浏览器安全验证页完成阿里云人机验证后自动发送短信。
    fn send_code(&mut self, cx: &mut Context<Self>) {
        // 若正在进行人机验证，再次点击则取消当前验证会话。
        if let Some(cancel) = self.verifying_cancel.take() {
            cancel.store(true, Ordering::SeqCst);
            self.set_message(MessageTone::Info, "已取消安全验证");
            cx.notify();
            return;
        }

        let phone = self.phone.read(cx).text().trim().to_string();
        if phone.is_empty() {
            self.set_message(MessageTone::Error, "请先输入手机号");
            cx.notify();
            return;
        }
        let now = epoch_secs();
        if now < self.last_send_epoch + SEND_CODE_COOLDOWN {
            let left = self.last_send_epoch + SEND_CODE_COOLDOWN - now;
            self.set_message(MessageTone::Info, format!("请 {left} 秒后再试"));
            cx.notify();
            return;
        }

        let cancel = Arc::new(AtomicBool::new(false));
        self.verifying_cancel = Some(cancel.clone());
        self.set_message(
            MessageTone::Info,
            "请在打开的浏览器页完成安全验证，验证通过后将自动发送短信…",
        );
        cx.notify();

        let auth = self.auth.clone();
        let cancel_bg = cancel.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    send_code_via_browser(auth, &phone, DEFAULT_AREA_CODE, &cancel_bg)
                })
                .await;

            let _ = this.update(cx, |view, cx| {
                // 若期间用户已取消并开启了新会话，则不覆盖新会话状态。
                let is_current = view
                    .verifying_cancel
                    .as_ref()
                    .map(|cur| Arc::ptr_eq(cur, &cancel))
                    .unwrap_or(false);
                if !is_current {
                    return;
                }
                view.verifying_cancel = None;

                match result {
                    Ok(()) => {
                        view.last_send_epoch = epoch_secs();
                        view.set_message(MessageTone::Success, "验证码已发送，请查收短信");
                        cx.activate(true);
                        view.start_cooldown_ticker(cx);
                    }
                    Err(e) => {
                        let detail = format!("{e:#}");
                        if detail.contains("已取消安全验证") {
                            view.set_message(MessageTone::Info, "已取消安全验证");
                        } else {
                            view.set_message(MessageTone::Error, format!("发送失败：{detail}"));
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 验证码登录（后台线程请求，避免阻塞 UI）。
    fn do_login(&mut self, cx: &mut Context<Self>) {
        if self.logging_in {
            return;
        }
        let phone = self.phone.read(cx).text().trim().to_string();
        let code = self.code.read(cx).text().trim().to_string();
        if phone.is_empty() || code.is_empty() {
            self.set_message(MessageTone::Error, "请输入手机号与验证码");
            cx.notify();
            return;
        }

        // 登录前若仍在等待验证码安全验证，顺带回收验证监听。
        if let Some(cancel) = self.verifying_cancel.take() {
            cancel.store(true, Ordering::SeqCst);
        }

        self.logging_in = true;
        self.set_message(MessageTone::Info, "登录中…");
        cx.notify();

        let auth = self.auth.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut guard = auth
                        .lock()
                        .map_err(|_| anyhow::anyhow!("认证管理器不可用"))?;
                    guard.login(&phone, &code, DEFAULT_AREA_CODE)
                })
                .await;

            let _ = this.update(cx, |view, cx| {
                view.logging_in = false;
                match result {
                    Ok(()) => {
                        view.phase = Phase::LoggedIn;
                        view.message.clear();
                    }
                    Err(e) => {
                        view.set_message(MessageTone::Error, format!("登录失败：{e:#}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 退出登录，回到表单。
    fn logout(&mut self, cx: &mut Context<Self>) {
        if let Ok(mut auth) = self.auth.lock() {
            if let Err(e) = auth.logout() {
                log::warn!("退出登录时清理会话失败：{e:#}");
            }
        }
        self.phase = Phase::Form;
        self.message.clear();
        cx.notify();
    }

    /// 「发送验证码」按钮。
    fn send_button(&self, cx: &mut Context<Self>, cooldown_left: u64) -> Stateful<Div> {
        let verifying = self.verifying_cancel.is_some();
        let label = if verifying {
            "取消验证".to_string()
        } else if cooldown_left > 0 {
            format!("{cooldown_left}s 后重发")
        } else {
            "发送验证码".to_string()
        };
        div()
            .id("send-code-btn")
            .on_click(cx.listener(|this, _e, _window, cx| this.send_code(cx)))
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(if verifying { 0x3A2E2E } else { 0x2A2A2A }))
            .text_color(rgb(0xFFFFFF))
            .text_sm()
            .cursor_pointer()
            .child(label)
    }

    /// 「登录」按钮。
    fn login_button(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let label = if self.logging_in { "登录中…" } else { "登录" };
        div()
            .id("login-btn")
            .on_click(cx.listener(|this, _e, _window, cx| this.do_login(cx)))
            .px_4()
            .py_2()
            .rounded_md()
            .bg(rgb(0xE6B84C))
            .text_color(rgb(0x141414))
            .text_base()
            .cursor_pointer()
            .child(label)
    }

    /// 「退出登录」按钮。
    fn logout_button(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("logout-btn")
            .on_click(cx.listener(|this, _e, _window, cx| this.logout(cx)))
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(0x2A2A2A))
            .text_color(rgb(0xFFFFFF))
            .text_sm()
            .cursor_pointer()
            .child("退出登录")
    }

    fn render_form(&mut self, cx: &mut Context<Self>) -> Div {
        let phone = self.phone.clone();
        let code = self.code.clone();
        let message = self.message.clone();
        let tone_color = match self.message_tone {
            MessageTone::Info => 0xBBBBBB,
            MessageTone::Success => 0x6CBF84,
            MessageTone::Error => 0xE06C5B,
        };
        let now = epoch_secs();
        let cooldown_left = if now < self.last_send_epoch + SEND_CODE_COOLDOWN {
            self.last_send_epoch + SEND_CODE_COOLDOWN - now
        } else {
            0
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .bg(rgb(0x141414))
            .text_color(rgb(0xFFFFFF))
            .child(
                div()
                    .text_lg()
                    .text_color(rgb(0xE6B84C))
                    .child("小宇宙 · 登录"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0x999999))
                    .child("使用手机号验证码登录"),
            )
            .child(
                div()
                    .w(px(360.))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(phone)
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(div().flex_1().child(code))
                            .child(self.send_button(cx, cooldown_left)),
                    ),
            )
            .child(if message.is_empty() {
                div()
            } else {
                div()
                    .max_w(px(480.))
                    .text_sm()
                    .text_color(rgb(tone_color))
                    .child(message)
            })
            .child(self.login_button(cx))
            .child(
                div()
                    .w(px(360.))
                    .text_xs()
                    .text_color(rgb(0x666666))
                    .child(DISCLAIMER),
            )
    }

    fn render_logged_in(&mut self, cx: &mut Context<Self>) -> Div {
        let nickname = self.nickname();
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .bg(rgb(0x141414))
            .text_color(rgb(0xFFFFFF))
            .child(
                div()
                    .text_lg()
                    .text_color(rgb(0xE6B84C))
                    .child("登录成功"),
            )
            .child(div().text_base().child(format!("欢迎，{nickname}")))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0x999999))
                    .child("首页 / 发现 / 搜索等页面将在后续里程碑实现"),
            )
            .child(self.logout_button(cx))
    }
}

impl gpui::Render for LoginView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.phase {
            Phase::LoggedIn => self.render_logged_in(cx),
            Phase::Form => self.render_form(cx),
        }
    }
}
