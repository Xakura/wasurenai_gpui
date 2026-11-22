//! Windows 构筑脚本：纯 Rust 生成多分辨率极简几何图标并直接写入 `.res` 资源文件，
//! 由 MSVC `link.exe` 内置 `cvtres` 链接进可执行文件（无需外部 `rc.exe`）。

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let res_path = out_dir.join("wasurenai_icon.res");

    let res_bytes = build_icon_res(&[16, 32, 48, 64]);
    fs::write(&res_path, res_bytes).expect("write wasurenai_icon.res");

    println!("cargo:rustc-link-arg-bins={}", res_path.display());
}

fn build_icon_res(sizes: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    // 强制 32 字节空头（标识 32-bit .res 格式）
    push_res_header(&mut out, 0, 0, 0, 0, 0);

    let mut group_entries = Vec::new();
    for (idx, &size) in sizes.iter().enumerate() {
        let id = (idx + 1) as u16;
        let dib = build_icon_dib(size);
        group_entries.push((size, id, dib.len() as u32));

        // RT_ICON = 3
        push_res_header(&mut out, dib.len() as u32, 3, id, 0x1010, 0x0409);
        out.extend_from_slice(&dib);
        align_dword(&mut out);
    }

    // RT_GROUP_ICON = 14, 资源 ID = 1
    let mut group = Vec::with_capacity(6 + group_entries.len() * 14);
    group.extend_from_slice(&0u16.to_le_bytes()); // idReserved
    group.extend_from_slice(&1u16.to_le_bytes()); // idType = Icon
    group.extend_from_slice(&(group_entries.len() as u16).to_le_bytes());
    for (size, id, bytes_in_res) in group_entries {
        let dim = if size >= 256 { 0 } else { size as u8 };
        group.push(dim); // bWidth
        group.push(dim); // bHeight
        group.push(0); // bColorCount
        group.push(0); // bReserved
        group.extend_from_slice(&1u16.to_le_bytes()); // wPlanes
        group.extend_from_slice(&32u16.to_le_bytes()); // wBitCount
        group.extend_from_slice(&bytes_in_res.to_le_bytes());
        group.extend_from_slice(&id.to_le_bytes());
    }

    push_res_header(&mut out, group.len() as u32, 14, 1, 0x1030, 0x0409);
    out.extend_from_slice(&group);
    align_dword(&mut out);

    out
}

fn push_res_header(
    out: &mut Vec<u8>,
    data_size: u32,
    type_id: u16,
    name_id: u16,
    memory_flags: u16,
    language_id: u16,
) {
    out.extend_from_slice(&data_size.to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes()); // HeaderSize
    out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    out.extend_from_slice(&type_id.to_le_bytes());
    out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    out.extend_from_slice(&name_id.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // DataVersion
    out.extend_from_slice(&memory_flags.to_le_bytes());
    out.extend_from_slice(&language_id.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // Version
    out.extend_from_slice(&0u32.to_le_bytes()); // Characteristics
}

fn align_dword(out: &mut Vec<u8>) {
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

/// 生成单个尺寸的 32-bit BGRA DIB（BITMAPINFOHEADER + 自底向上 BGRA + 1bpp AND mask）。
fn build_icon_dib(size: u32) -> Vec<u8> {
    let rgba = render_rgba_icon(size);
    let xor_len = (size * size * 4) as usize;
    let and_stride = size.div_ceil(32) * 4;
    let and_len = (and_stride * size) as usize;

    let mut dib = Vec::with_capacity(40 + xor_len + and_len);
    // BITMAPINFOHEADER (40 bytes)
    dib.extend_from_slice(&40u32.to_le_bytes()); // biSize
    dib.extend_from_slice(&(size as i32).to_le_bytes()); // biWidth
    dib.extend_from_slice(&((size * 2) as i32).to_le_bytes()); // biHeight (XOR + AND)
    dib.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    dib.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    dib.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
    dib.extend_from_slice(&(xor_len as u32).to_le_bytes()); // biSizeImage
    dib.extend_from_slice(&0i32.to_le_bytes()); // biXPelsPerMeter
    dib.extend_from_slice(&0i32.to_le_bytes()); // biYPelsPerMeter
    dib.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    dib.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant

    // XOR 像素（自底向上 BGRA）
    for y in (0..size).rev() {
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            dib.push(rgba[i + 2]); // B
            dib.push(rgba[i + 1]); // G
            dib.push(rgba[i]); // R
            dib.push(rgba[i + 3]); // A
        }
    }

    // AND 掩码（自底向上 1bpp，透明位为 1）
    for y in (0..size).rev() {
        let mut row = vec![0u8; and_stride as usize];
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            if rgba[i + 3] == 0 {
                row[(x / 8) as usize] |= 0x80 >> (x % 8);
            }
        }
        dib.extend_from_slice(&row);
    }

    dib
}

/// 与 `src/platform/icon.rs` 相同的极简几何待办图标 SDF 渲染。
fn render_rgba_icon(size: u32) -> Vec<u8> {
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    let px_w = 2.0 / size as f32;
    let half = 0.84_f32;
    let radius = 0.40_f32;
    let stroke_r = 0.145_f32;

    for y in 0..size {
        let v = ((y as f32 + 0.5) / size as f32) * 2.0 - 1.0;
        for x in 0..size {
            let u = ((x as f32 + 0.5) / size as f32) * 2.0 - 1.0;

            let qx = u.abs() - (half - radius);
            let qy = v.abs() - (half - radius);
            let box_sdf =
                (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - radius;
            let box_cov = smooth_cov(box_sdf, px_w);

            let t = ((u + v) / (2.0 * half) * 0.5 + 0.5).clamp(0.0, 1.0);
            let mut r = lerp(37.0, 29.0, t);
            let mut g = lerp(99.0, 78.0, t);
            let mut b = lerp(235.0, 216.0, t);

            let d1 = segment_dist(u, v, (-0.42, 0.02), (-0.12, 0.34));
            let d2 = segment_dist(u, v, (-0.12, 0.34), (0.44, -0.32));
            let check_cov = smooth_cov(d1.min(d2) - stroke_r, px_w) * box_cov;
            if check_cov > 0.0 {
                r = lerp(r, 255.0, check_cov);
                g = lerp(g, 255.0, check_cov);
                b = lerp(b, 255.0, check_cov);
            }

            rgba.push(r.round().clamp(0.0, 255.0) as u8);
            rgba.push(g.round().clamp(0.0, 255.0) as u8);
            rgba.push(b.round().clamp(0.0, 255.0) as u8);
            rgba.push((box_cov * 255.0).round().clamp(0.0, 255.0) as u8);
        }
    }

    rgba
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
