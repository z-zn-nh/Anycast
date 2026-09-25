#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;
mod gui;
mod models;

slint::include_modules!();

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // 索引维护命令行：命中即执行并退出，不启动 GUI
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = core::cli::maybe_run(&argv) {
        std::process::exit(code);
    }

    let silent = std::env::args().any(|a| a == "--silent" || a == "--tray");
    // 渲染器选择：Windows 上 Skia(wgpu/D3D12) 的 HWND 交换链不支持预乘 Alpha（窗口不透明），
    // OpenGL 路径在部分 NVIDIA 驱动（DXGI 分层呈现）下透明窗口整窗不可见；
    // 软件渲染器经 softbuffer 输出预乘 BGRA，配合 DWM Acrylic backdrop 可稳定获得亚克力效果。
    // 可用环境变量 SLINT_BACKEND 覆盖（如 winit-skia-opengl）。
    if std::env::var_os("SLINT_BACKEND").is_none() {
        if let Err(e) = slint::BackendSelector::new().renderer_name("software".into()).select() {
            log::warn!("选择 software 渲染器失败，使用默认渲染器: {e}");
        }
    }
    if let Err(e) = gui::run(silent) {
        log::error!("Anycast 启动失败: {e:#}");
        std::process::exit(1);
    }
}
