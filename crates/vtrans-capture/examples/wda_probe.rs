#![allow(unsafe_code)] // Windows interop requires unsafe; each block below has a SAFETY comment.

//! WDA x WGC 显示器级捕获验证探针
//!
//! 本探针在真实 Windows 桌面上实证一个关键问题: `SetWindowDisplayAffinity`
//! 的 `WDA_EXCLUDEFROMCAPTURE` 对 `vtrans-capture` 所使用的 Windows Graphics
//! Capture **显示器级** 捕获 (`GraphicsCaptureItem::CreateForMonitor`) 是否
//! 生效 —— 即被标记的窗口在捕获帧中是消失/变黑, 还是仍然可见。该行为在
//! 不同资料中记载矛盾, 必须以实机结果为准, 不能凭文档臆断。
//!
//! # 流程 (自包含, 运行数秒, 需要交互式 Windows 桌面会话)
//!
//! 1. 显式设置 `Per-Monitor V2` DPI awareness, 之后所有 Win32 坐标均为物理像素;
//! 2. 通过公开 API 创建 [`WindowsCaptureSource`](vtrans_capture::WindowsCaptureSource);
//! 3. 在测试窗口出现前先捕获目标区域一次 (baseline, 期望约 0 红色像素);
//! 4. 在主显示器上创建一个置顶的纯红色小窗口, 再次捕获 (期望大量红色);
//! 5. 对窗口调用 `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`, 并用
//!    `GetWindowDisplayAffinity` 回读确认;
//! 6. 再捕获两次并比较红色像素占比: 红色消失/变黑 → WDA 被 WGC 显示器捕获
//!    尊重; 红色仍在 → WDA 对 WGC 显示器捕获无效。
//!
//! # 运行
//!
//! ```powershell
//! cargo run -p vtrans-capture --example wda_probe
//! ```
//!
//! 运行期间主显示器上会短暂出现一个红色小窗口, 属预期现象。窗口创建、
//! 捕获初始化均带失败容错, 窗口句柄由 RAII 结构负责销毁与注销。

use std::time::Duration;

use vtrans_capture::{MonitorInfo, WindowsCaptureSource};
use vtrans_core::traits::CaptureSource;
use vtrans_core::types::{CapturedImage, ScreenRegion};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, UpdateWindow, HBRUSH, HGDIOBJ,
    PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetWindowDisplayAffinity,
    PeekMessageW, PostQuitMessage, RegisterClassW, SetWindowDisplayAffinity, SetWindowPos,
    TranslateMessage, UnregisterClassW, CS_HREDRAW, CS_VREDRAW, HCURSOR, HICON, HMENU,
    HWND_TOPMOST, MSG, PM_REMOVE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WDA_EXCLUDEFROMCAPTURE,
    WINDOW_DISPLAY_AFFINITY, WINDOW_EX_STYLE, WM_DESTROY, WM_PAINT, WM_QUIT, WNDCLASSW, WS_POPUP,
    WS_VISIBLE,
};

/// 探针窗口类名与窗口标题 (`RegisterClassW` / `CreateWindowExW` 共用)。
const WINDOW_CLASS_NAME: PCWSTR = w!("vtrans-wda-probe");

/// 窗口在显示器相对坐标中的位置 (物理像素)。
const WINDOW_OFFSET_X: i32 = 150;
const WINDOW_OFFSET_Y: i32 = 150;

/// 窗口尺寸 (物理像素)。
const WINDOW_W: i32 = 220;
const WINDOW_H: i32 = 160;

/// 捕获区域 (显示器相对坐标, 物理像素), 完全覆盖探针窗口。
const REGION_X: i32 = 100;
const REGION_Y: i32 = 100;
const REGION_W: u32 = 340;
const REGION_H: u32 = 260;

/// 纯红色 `COLORREF` (格式 `0x00BBGGRR`)。
const RED: COLORREF = COLORREF(0x0000_00FF);

/// "红色像素" 判定阈值 (BGRA, 8-bit)。
const RED_MIN_R: u8 = 200;
const RED_MAX_G: u8 = 80;
const RED_MAX_B: u8 = 80;

/// baseline 红色占比高于该千分比时, 环境不够中性, 结论标注警告。
const BASELINE_NEUTRAL_WARN_PERMILLE: u64 = 50;

/// 窗口可见判定: 窗口带来的红色增量 (千分比) 至少达到该值, 否则探针失败。
const VISIBLE_MIN_PERMILLE: u64 = 150;

/// WDA 生效判定: 红色回落到 baseline + max(该值, 窗口增量的 5%) 以内。
const HONORED_ABS_PERMILLE: u64 = 20;

/// 实机验证结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// WDA 被 WGC 显示器捕获尊重: 窗口从捕获帧中消失 (变黑/露出背景)。
    Honored,
    /// WDA 对 WGC 显示器捕获无效: 窗口仍出现在捕获帧中。
    Ignored,
    /// 红色占比只下降了一部分, 环境干扰导致无法下结论。
    Inconclusive,
}

/// 一次捕获帧的红色像素统计。
struct RedStats {
    /// 区域像素总数。
    total: u64,
    /// 判定为红色的像素数。
    red: u64,
    /// 红色像素占比 (千分比, 0-1000)。
    red_permille: u64,
    /// 窗口子区域的平均 RGB 颜色, 用于描述窗口被替换成了什么。
    window_mean_rgb: (u64, u64, u64),
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== WDA x WGC monitor-capture probe ===");

    // SAFETY: 上下文句柄是进程级常量, 无外部状态要求。
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let source = WindowsCaptureSource::new()?;
    let primary = source
        .list_monitors()
        .into_iter()
        .find(|m| m.is_primary)
        .ok_or("no primary monitor found")?;
    check_geometry(&primary)?;

    let region = ScreenRegion::new(primary.id.clone(), REGION_X, REGION_Y, REGION_W, REGION_H);
    let window_rel = window_rect_in_region();

    println!(
        "monitor: {} ({}x{} physical px, scale={})",
        primary.id, primary.width, primary.height, primary.scale_factor
    );
    println!(
        "region : monitor-relative ({REGION_X}, {REGION_Y}) {REGION_W}x{REGION_H} physical px"
    );

    // 1. baseline: 窗口尚未创建, 期望红色占比约 0。
    let baseline_img = capture(&source, &region).await?;
    let baseline = analyze(&baseline_img, window_rel);
    print_stats("[baseline]", &baseline);
    if baseline.red_permille > BASELINE_NEUTRAL_WARN_PERMILLE {
        println!(
            "WARNING: baseline region is not red-neutral ({} permille); verdict reliability reduced",
            baseline.red_permille
        );
    }

    // 2. 创建红色测试窗口, 等合成器稳定后再次捕获, 期望大量红色。
    let window = create_probe_window(&primary)?;
    println!(
        "window : hwnd={:?} at monitor-relative ({WINDOW_OFFSET_X}, {WINDOW_OFFSET_Y}) {WINDOW_W}x{WINDOW_H}",
        window.hwnd()
    );
    pump_messages(&window).await;
    tokio::time::sleep(Duration::from_millis(400)).await;

    let visible_img = capture(&source, &region).await?;
    let visible = analyze(&visible_img, window_rel);
    print_stats("[visible]", &visible);

    let window_added = visible.red_permille.saturating_sub(baseline.red_permille);
    println!("[visible] window red contribution = {window_added} permille");
    if window_added < VISIBLE_MIN_PERMILLE {
        return Err(format!(
            "probe failed: test window not visible in WGC monitor capture \
             (red contribution {window_added} permille)"
        )
        .into());
    }

    // 3. 应用 WDA_EXCLUDEFROMCAPTURE 并回读确认。
    let affinity = apply_wda(window.hwnd())?;
    println!(
        "WDA     : SetWindowDisplayAffinity -> {}, readback = {} (expected {})",
        WDA_EXCLUDEFROMCAPTURE.0, affinity.0, WDA_EXCLUDEFROMCAPTURE.0
    );

    // 4. 两次 WDA 后的捕获, 取红色占比较高者做结论, 减少单帧时序抖动。
    tokio::time::sleep(Duration::from_millis(600)).await;
    let after_first_img = capture(&source, &region).await?;
    let after_first = analyze(&after_first_img, window_rel);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let after_second_img = capture(&source, &region).await?;
    let after_second = analyze(&after_second_img, window_rel);
    print_stats("[after 1]", &after_first);
    print_stats("[after 2]", &after_second);

    let after = after_first.red_permille.max(after_second.red_permille);
    let after_above_baseline = after.saturating_sub(baseline.red_permille);

    // 5. 结论。
    let honored_cap = HONORED_ABS_PERMILLE.max(window_added / 20);
    let verdict = if after <= baseline.red_permille.saturating_add(honored_cap) {
        Verdict::Honored
    } else if after >= baseline.red_permille + window_added / 2 {
        Verdict::Ignored
    } else {
        Verdict::Inconclusive
    };

    match verdict {
        Verdict::Honored => println!(
            "VERDICT: WDA_EXCLUDEFROMCAPTURE honored by WGC monitor capture: YES \
             (window red {window_added} -> {after_above_baseline} permille above baseline)"
        ),
        Verdict::Ignored => println!(
            "VERDICT: WDA_EXCLUDEFROMCAPTURE honored by WGC monitor capture: NO \
             (window red still {after_above_baseline} of {window_added} permille above baseline)"
        ),
        Verdict::Inconclusive => println!(
            "VERDICT: INCONCLUSIVE (red dropped only partially: \
             {window_added} -> {after_above_baseline} permille above baseline)"
        ),
    }

    // `window` 在此作用域结束时 drop: 销毁窗口、释放画刷并注销窗口类。
    Ok(())
}

/// 校验主显示器足够容纳捕获区域与探针窗口。
fn check_geometry(primary: &MonitorInfo) -> Result<(), Box<dyn std::error::Error>> {
    let needed_w = i64::from(REGION_X) + i64::from(REGION_W);
    let needed_h = i64::from(REGION_Y) + i64::from(REGION_H);
    if i64::from(primary.width) < needed_w || i64::from(primary.height) < needed_h {
        return Err(format!(
            "primary monitor too small for probe geometry: {}x{}, need at least {needed_w}x{needed_h}",
            primary.width, primary.height
        )
        .into());
    }
    Ok(())
}

/// 窗口在捕获区域内的相对矩形 `(x, y, w, h)` (物理像素)。
fn window_rect_in_region() -> (u32, u32, u32, u32) {
    (
        u32::try_from(WINDOW_OFFSET_X - REGION_X).expect("window offset inside region"),
        u32::try_from(WINDOW_OFFSET_Y - REGION_Y).expect("window offset inside region"),
        u32::try_from(WINDOW_W).expect("positive window width"),
        u32::try_from(WINDOW_H).expect("positive window height"),
    )
}

/// 单次捕获并映射错误。
async fn capture(
    source: &WindowsCaptureSource,
    region: &ScreenRegion,
) -> Result<CapturedImage, Box<dyn std::error::Error>> {
    source
        .capture_once(region)
        .await
        .map_err(|e| format!("capture_once failed: {e}").into())
}

/// 统计捕获帧中的红色像素占比与窗口子区域平均颜色。
fn analyze(image: &CapturedImage, window_rel: (u32, u32, u32, u32)) -> RedStats {
    let (win_left, win_top, win_width, win_height) = window_rel;
    let width = image.width as usize;
    let mut total = 0u64;
    let mut red = 0u64;
    let mut sum_r = 0u64;
    let mut sum_g = 0u64;
    let mut sum_b = 0u64;
    let mut window_pixels = 0u64;

    for row in 0..image.height {
        let row_index = row as usize;
        for col in 0..image.width {
            let col_index = col as usize;
            let i = (row_index * width + col_index) * 4;
            let b = image.data[i];
            let g = image.data[i + 1];
            let r = image.data[i + 2];
            total += 1;
            if r >= RED_MIN_R && g <= RED_MAX_G && b <= RED_MAX_B {
                red += 1;
            }
            if col >= win_left
                && col < win_left + win_width
                && row >= win_top
                && row < win_top + win_height
            {
                sum_r += u64::from(r);
                sum_g += u64::from(g);
                sum_b += u64::from(b);
                window_pixels += 1;
            }
        }
    }

    RedStats {
        total,
        red,
        red_permille: red * 1000 / total,
        window_mean_rgb: (
            mean(sum_r, window_pixels),
            mean(sum_g, window_pixels),
            mean(sum_b, window_pixels),
        ),
    }
}

/// 平均颜色分量, 样本为空时返回 0。
fn mean(sum: u64, count: u64) -> u64 {
    sum.checked_div(count).unwrap_or(0)
}

/// 打印一次捕获的统计信息。
fn print_stats(label: &str, stats: &RedStats) {
    println!(
        "{label} red={} total={} red_permille={} window_mean_rgb={:?}",
        stats.red, stats.total, stats.red_permille, stats.window_mean_rgb
    );
}

/// 拥有探针窗口生命周期的 RAII 句柄。
///
/// `drop` 时销毁窗口、释放画刷并注销窗口类, 保证异常路径同样清理。
#[derive(Debug)]
struct ProbeWindow {
    hwnd: HWND,
    brush: HBRUSH,
    instance: HINSTANCE,
}

impl ProbeWindow {
    /// 窗口句柄。
    const fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

impl Drop for ProbeWindow {
    fn drop(&mut self) {
        // SAFETY: `hwnd` 是本结构创建且尚未销毁的窗口句柄; `brush` 是注册
        // 窗口类时分配且未被其他地方释放的 GDI 画刷; `instance` 是进程主
        // 模块句柄, 进程生命周期内有效。
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            let _ = DeleteObject(HGDIOBJ(self.brush.0));
            let _ = UnregisterClassW(WINDOW_CLASS_NAME, self.instance);
        }
    }
}

/// 在主显示器上创建置顶的纯红色测试窗口。
fn create_probe_window(primary: &MonitorInfo) -> Result<ProbeWindow, String> {
    // SAFETY: 传入 null 表示当前进程主模块, 返回的句柄在进程生命周期内有效。
    let instance = unsafe { GetModuleHandleW(PCWSTR::null()) }
        .map_err(|e| format!("GetModuleHandleW failed: {e}"))?;
    let instance = HINSTANCE(instance.0);

    // SAFETY: 颜色为常量, 无指针或外部状态要求。
    let brush = unsafe { CreateSolidBrush(RED) };

    let wnd_class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(probe_wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: HICON(std::ptr::null_mut()),
        hCursor: HCURSOR(std::ptr::null_mut()),
        hbrBackground: brush,
        lpszMenuName: PCWSTR::null(),
        lpszClassName: WINDOW_CLASS_NAME,
    };

    // SAFETY: `wnd_class` 所有字段已初始化; 类名为静态 UTF-16 字面量。
    let atom = unsafe { RegisterClassW(&wnd_class) };
    if atom == 0 {
        // SAFETY: 注册失败时画刷不再需要, 立即释放避免泄漏。
        unsafe {
            let _ = DeleteObject(HGDIOBJ(brush.0));
        }
        return Err("RegisterClassW failed".to_string());
    }

    // SAFETY: 类名已成功注册; 坐标与尺寸有效; 父窗口与菜单为 null; 实例句柄有效。
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            WINDOW_CLASS_NAME,
            WINDOW_CLASS_NAME,
            WS_POPUP | WS_VISIBLE,
            primary.x + WINDOW_OFFSET_X,
            primary.y + WINDOW_OFFSET_Y,
            WINDOW_W,
            WINDOW_H,
            HWND::default(),
            HMENU(std::ptr::null_mut()),
            instance,
            None,
        )
    }
    .map_err(|e| format!("CreateWindowExW failed: {e}"))?;

    // 置顶但不激活 (不抢焦点), 位置尺寸不变, 然后强制同步重绘。
    // SAFETY: `hwnd` 刚创建有效; `HWND_TOPMOST` 为常量。
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
        let _ = UpdateWindow(hwnd);
    }

    Ok(ProbeWindow {
        hwnd,
        brush,
        instance,
    })
}

/// 对窗口应用 `WDA_EXCLUDEFROMCAPTURE` 并回读确认, 返回回读值。
fn apply_wda(hwnd: HWND) -> Result<WINDOW_DISPLAY_AFFINITY, String> {
    // SAFETY: `hwnd` 有效; `WDA_EXCLUDEFROMCAPTURE` 为合法常量。
    unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) }
        .map_err(|e| format!("SetWindowDisplayAffinity failed: {e}"))?;

    let mut affinity = 0u32;
    // SAFETY: `affinity` 为栈上 u32 输出参数, 由 API 写入。
    unsafe { GetWindowDisplayAffinity(hwnd, &mut affinity) }
        .map_err(|e| format!("GetWindowDisplayAffinity failed: {e}"))?;
    let affinity = WINDOW_DISPLAY_AFFINITY(affinity);
    if affinity.0 != WDA_EXCLUDEFROMCAPTURE.0 {
        return Err(format!(
            "WDA readback mismatch: expected {}, got {}",
            WDA_EXCLUDEFROMCAPTURE.0, affinity.0
        ));
    }
    Ok(affinity)
}

/// 泵空本窗口的消息队列, 确保重绘完成 (每轮 2 ms, 最多约 300 ms)。
async fn pump_messages(window: &ProbeWindow) {
    for _ in 0..150 {
        let mut msg = MSG::default();
        // SAFETY: `msg` 为栈上变量; `PM_REMOVE` 为常量; 只取本窗口消息。
        let has_msg = unsafe { PeekMessageW(&mut msg, window.hwnd(), 0, 0, PM_REMOVE) };
        if has_msg.as_bool() {
            if msg.message == WM_QUIT {
                break;
            }
            // SAFETY: 消息来自本线程队列且字段已由 `PeekMessageW` 填充。
            unsafe {
                let _ = TranslateMessage(&msg);
                let _ = DispatchMessageW(&msg);
            }
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

/// 探针窗口消息处理: `WM_PAINT` 用纯红色填充客户区, `WM_DESTROY` 退出消息循环。
unsafe extern "system" fn probe_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            // SAFETY: `BeginPaint` 与 `EndPaint` 必须在同一线程成对调用,
            // `ps` 为栈上变量, 生命周期覆盖整个绘制过程。
            let mut ps = PAINTSTRUCT::default();
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            // SAFETY: 画刷为本次绘制临时创建, 使用后立即释放。
            let brush = unsafe { CreateSolidBrush(RED) };
            unsafe {
                let _ = FillRect(hdc, &ps.rcPaint, brush);
                let _ = DeleteObject(HGDIOBJ(brush.0));
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            // SAFETY: 无参数依赖, 向本线程消息队列投递退出消息。
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
