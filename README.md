# Stardust

小宇宙（Xiaoyuzhou FM）的跨平台桌面客户端，使用 [GPUI](https://www.gpui.rs/) 构建。

## 架构

- 客户端通过 `stardust-api` 直接请求小宇宙上游 API（`https://api.xiaoyuzhoufm.com`），
  不再内嵌或依赖任何本地代理服务。
- 请求头、请求体与 token 处理对齐官方 iOS 客户端行为；登录 / 刷新接口从上游
  响应头提取 `x-jike-access-token` / `x-jike-refresh-token`。
- 仓库为多 crate 分层组织（对齐 [sonora](https://github.com/sonorahq/sonora)）：

  | crate | 职责 |
  | --- | --- |
  | `stardust-api` | 上游 API 的直连 HTTP 客户端与数据模型 |
  | `stardust-state` | 会话持久化、认证管理与 token 刷新 |
  | `stardust-views` | 页面视图（登录页等） |
  | `stardust-ui` / `stardust-router` | UI 组件与路由 |
  | `stardust` | 可执行入口 |

## 构建

仅需 Rust 工具链（无需安装 Go）：

```bash
cargo build --release
```

运行测试：

```bash
cargo test --workspace
```

## 运行

```bash
cargo run --release
```

## 使用说明

- 首次使用需通过短信验证码登录小宇宙账号。
- 登录成功后，会话（access / refresh token）会持久化到本地配置目录，并在后台
  自动刷新，token 失效时无需重复登录。

## 免责声明

- 本项目为第三方客户端，与小宇宙（Xiaoyuzhou FM）官方无关。登录与数据访问均
  通过官方公开接口进行，请遵守小宇宙的服务条款。
- 本项目仅用于学习与个人备份用途，请勿用于批量抓取、绕过风控或任何侵犯著作权
  的行为。因使用本项目产生的账号风险（如封禁、限流）或法律风险由使用者自行承担。
- 音频版权归原作者与小宇宙平台所有，请支持正版。
