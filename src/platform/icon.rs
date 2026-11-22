//! 极简几何待办图标：矢量 SDF 抗锯齿渲染（托盘图标）与窗口图标加载。

use tray_icon::Icon;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_SHARED, LoadImageW, SM_CXICON,
    SM_CXSMICON, SM_CYICON, SM_CYSMICON, SendMessageW, WM_SETICON,
};
use windows::core::PCWSTR;

const DOT_AMBER: [u8; 3] = [0xf5, 0x9e, 0x0b];
const DOT_RED: [u8; 3] = [0xef, 0x44, 0x44];

/// 生成指定像素尺寸的极简几何待办图标（直通 RGBA，自上而下）。
///
/// `badge = true` 时在右上角叠加带白边的小圆点，表示还有待办。
pub fn render_rgba(size: u32, badge: bool) -> Vec<u8> {
    render_rgba_dot(size, badge.then_some(DOT_AMBER))
}

fn render_rgba_dot(size: u32, dot: Option<[u8; 3]>) -> Vec<u8> {
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    if size == 0 {
        return rgba;
    }

    let px_w = 2.0 / size as f32;
    let half = 0.84_f32;
    let radius = 0.40_f32;
    let stroke_r = 0.145_f32;

    for y in 0..size {
        let v = ((y as f32 + 0.5) / size as f32) * 2.0 - 1.0;
        for x in 0..size {
            let u = ((x as f32 + 0.5) / size as f32) * 2.0 - 1.0;

            let box_sdf = rounded_box_sdf(u, v, half, radius);
            let box_cov = smooth_cov(box_sdf, px_w);

            // 从左上 (#2563EB) 到右下 (#1D4ED8) 的柔和钴蓝渐变
            let t = ((u + v) / (2.0 * half) * 0.5 + 0.5).clamp(0.0, 1.0);
            let mut r = lerp(37.0, 29.0, t);
            let mut g = lerp(99.0, 78.0, t);
            let mut b = lerp(235.0, 216.0, t);
            let mut a = box_cov;

            // 居中纯白圆头对勾（两段胶囊 SDF 合并）
            let d1 = segment_dist(u, v, (-0.42, 0.02), (-0.12, 0.34));
            let d2 = segment_dist(u, v, (-0.12, 0.34), (0.44, -0.32));
            let check_cov = smooth_cov(d1.min(d2) - stroke_r, px_w) * box_cov;
            if check_cov > 0.0 {
                r = lerp(r, 255.0, check_cov);
                g = lerp(g, 255.0, check_cov);
                b = lerp(b, 255.0, check_cov);
            }

            // 右上角提示点（带纯白隔离环）
            if let Some(dot_rgb) = dot {
                let dist = ((u - 0.62).powi(2) + (v + 0.62).powi(2)).sqrt();
                let ring_cov = smooth_cov(dist - 0.31, px_w);
                let dot_cov = smooth_cov(dist - 0.21, px_w);
                if ring_cov > 0.0 {
                    let out_a = a + ring_cov * (1.0 - a);
                    if out_a > 0.0 {
                        r = (r * a * (1.0 - ring_cov) + 255.0 * ring_cov) / out_a;
                        g = (g * a * (1.0 - ring_cov) + 255.0 * ring_cov) / out_a;
                        b = (b * a * (1.0 - ring_cov) + 255.0 * ring_cov) / out_a;
                        a = out_a;
                    }
                }
                if dot_cov > 0.0 {
                    r = lerp(r, dot_rgb[0] as f32, dot_cov);
                    g = lerp(g, dot_rgb[1] as f32, dot_cov);
                    b = lerp(b, dot_rgb[2] as f32, dot_cov);
                }
            }

            rgba.push(r.round().clamp(0.0, 255.0) as u8);
            rgba.push(g.round().clamp(0.0, 255.0) as u8);
            rgba.push(b.round().clamp(0.0, 255.0) as u8);
            rgba.push((a * 255.0).round().clamp(0.0, 255.0) as u8);
        }
    }

    rgba
}

/// 托盘图标：32×32，有逾期用红点，有未完成用琥珀点，全部完成为纯净蓝底白勾。
pub fn tray_icon(pending: usize, overdue: usize) -> Icon {
    let dot = if overdue > 0 {
        Some(DOT_RED)
    } else if pending > 0 {
        Some(DOT_AMBER)
    } else {
        None
    };
    let rgba = render_rgba_dot(32, dot);
    Icon::from_rgba(rgba, 32, 32).expect("内置托盘图标 RGBA 数据合法")
}

/// 从 exe 内嵌资源（ID = 1）加载图标并应用到窗口标题栏与任务栏。
///
/// 在未链接 `.res` 资源的环境（如某些示例）下静默降级，不报错。
pub fn apply_window_icon(hwnd: HWND) {
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            return;
        };
        let hinst = HINSTANCE(module.0);
        // Win32 MAKEINTRESOURCEW(1)：把整数资源 ID 1 编进低 16 位指针传入 LoadImageW
        let res_id = PCWSTR(std::ptr::without_provenance(1));

        let small_w = GetSystemMetrics(SM_CXSMICON).max(16);
        let small_h = GetSystemMetrics(SM_CYSMICON).max(16);
        if let Ok(handle) = LoadImageW(Some(hinst), res_id, IMAGE_ICON, small_w, small_h, LR_SHARED)
        {
            SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_SMALL as usize)),
                Some(LPARAM(handle.0 as isize)),
            );
        }

        let big_w = GetSystemMetrics(SM_CXICON).max(32);
        let big_h = GetSystemMetrics(SM_CYICON).max(32);
        if let Ok(handle) = LoadImageW(Some(hinst), res_id, IMAGE_ICON, big_w, big_h, LR_SHARED) {
            SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_BIG as usize)),
                Some(LPARAM(handle.0 as isize)),
            );
        }
    }
}

fn rounded_box_sdf(u: f32, v: f32, half: f32, radius: f32) -> f32 {
    let qx = u.abs() - (half - radius);
    let qy = v.abs() - (half - radius);
    let out_len = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    out_len + qx.max(qy).min(0.0) - radius
}

fn segment_dist(px: f32, py: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let pax = px - a.0;
    let pay = py - a.1;
    let bax = b.0 - a.0;
    let bay = b.1 - a.1;
    let denom = bax * bax + bay * bay;
    let h = if denom > 0.0 {
        ((pax * bax + pay * bay) / denom).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((pax - bax * h).powi(2) + (pay - bay * h).powi(2)).sqrt()
}

fn smooth_cov(sdf: f32, px_w: f32) -> f32 {
    let half_w = px_w * 0.75;
    ((-sdf + half_w) / (2.0 * half_w)).clamp(0.0, 1.0)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_rgba_dimensions_and_alpha() {
        let size = 32;
        let plain = render_rgba(size, false);
        assert_eq!(plain.len(), (size * size * 4) as usize);

        // 四角应完全透明（圆角外部）
        let corner_alpha = plain[3];
        assert_eq!(corner_alpha, 0, "左上角圆角外部应完全透明");

        // 中心应完全不透明
        let center_idx = (((size / 2) * size + (size / 2)) * 4) as usize;
        assert_eq!(plain[center_idx + 3], 255, "图标中心应完全不透明");

        // 带角标时右上角附近应有非零像素
        let badged = render_rgba(size, true);
        assert_eq!(badged.len(), plain.len());
        let top_right_idx = ((5 * size + 26) * 4) as usize;
        assert!(badged[top_right_idx + 3] > 0, "开启 badge 时右上角应绘制提示点");
    }
}
