use std::path::{Path, PathBuf};
use std::process::Command;

/// 构建脚本：将 vendor/xyz 的 Go 源码编译为当前目标平台的二进制，输出到 OUT_DIR。
///
/// 主程序通过 `include_bytes!(concat!(env!("OUT_DIR"), "/xyz"))` 内嵌该二进制，
/// 运行时再提取到临时目录作为子进程拉起。
fn main() {
    // xyz 源码或本脚本变化时重新构建
    println!("cargo:rerun-if-changed=../../vendor/xyz");
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();

    // Rust 目标三元组 → Go 的 GOOS/GOARCH
    let goos = match target_os.as_str() {
        "macos" => "darwin",
        "windows" => "windows",
        "linux" => "linux",
        other => other,
    };
    let goarch = match target_arch.as_str() {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        "x86" => "386",
        other => other,
    };

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let out = out_dir.join(if goos == "windows" { "xyz.exe" } else { "xyz" });

    // 已存在则跳过（cargo 缓存 + 手动构建均可用）
    if out.exists() {
        return;
    }

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let vendor_dir = Path::new(&manifest_dir).join("../../vendor/xyz");

    let status = Command::new("go")
        .arg("build")
        .arg("-trimpath")
        .arg("-ldflags")
        .arg("-s -w")
        .arg("-o")
        .arg(&out)
        .arg(".")
        .current_dir(&vendor_dir)
        .env("CGO_ENABLED", "0")
        .env("GOOS", goos)
        .env("GOARCH", goarch)
        .status()
        .expect("无法执行 `go build`：请确认已安装 Go 工具链（用于编译内嵌的 xyz 服务）");

    if !status.success() {
        panic!("go build 失败：无法编译内嵌的 xyz 服务");
    }

    println!("cargo:warning=已构建内嵌 xyz 服务 ({goos}/{goarch}) -> {}", out.display());
}
