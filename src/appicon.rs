//! The app icon, drawn in code so the window, the exe and the release
//! artwork all come from one definition. Also compiled by build.rs, so it
//! must not depend on anything else in the crate.

const TOP: [f32; 3] = [46.0, 149.0, 255.0];
const BOTTOM: [f32; 3] = [0.0, 98.0, 214.0];

fn in_rounded_square(x: f32, y: f32, m: f32, r: f32) -> bool {
    let (lo, hi) = (m, 1.0 - m);
    if x < lo || x > hi || y < lo || y > hi {
        return false;
    }
    let cx = x.clamp(lo + r, hi - r);
    let cy = y.clamp(lo + r, hi - r);
    (x - cx).powi(2) + (y - cy).powi(2) <= r * r
}

fn in_glyph(x: f32, y: f32) -> bool {
    let d = ((x - 0.5).powi(2) + (y - 0.5).powi(2)).sqrt();
    (d - 0.25).abs() <= 0.036 || d <= 0.11
}

/// Straight RGBA pixels, row by row from the top.
pub fn rgba(size: u32) -> Vec<u8> {
    const SS: u32 = 4;
    let mut out = vec![0u8; (size * size * 4) as usize];
    let margin = 0.04;
    let radius = 0.225 * (1.0 - 2.0 * margin);
    for py in 0..size {
        for px in 0..size {
            let mut acc = [0f32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = (px as f32 + (sx as f32 + 0.5) / SS as f32) / size as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / SS as f32) / size as f32;
                    if !in_rounded_square(x, y, margin, radius) {
                        continue;
                    }
                    let c = if in_glyph(x, y) {
                        [255.0; 3]
                    } else {
                        let t = ((y - margin) / (1.0 - 2.0 * margin)).clamp(0.0, 1.0);
                        [0, 1, 2].map(|i| TOP[i] + (BOTTOM[i] - TOP[i]) * t)
                    };
                    for i in 0..3 {
                        acc[i] += c[i];
                    }
                    acc[3] += 1.0;
                }
            }
            if acc[3] == 0.0 {
                continue;
            }
            let i = ((py * size + px) * 4) as usize;
            for c in 0..3 {
                out[i + c] = (acc[c] / acc[3]).round() as u8;
            }
            out[i + 3] = (acc[3] * 255.0 / (SS * SS) as f32).round() as u8;
        }
    }
    out
}

/// A Windows .ico holding the icon at several sizes.
#[allow(dead_code)]
pub fn ico(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes.iter().map(|&s| bmp_entry(s)).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (s, img) in sizes.iter().zip(&images) {
        let dim = if *s >= 256 { 0 } else { *s as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0, 1, 0, 32, 0]);
        out.extend_from_slice(&(img.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for img in images {
        out.extend_from_slice(&img);
    }
    out
}

fn bmp_entry(size: u32) -> Vec<u8> {
    let px = rgba(size);
    let mask_row = (size.div_ceil(32) * 4) as usize;
    let mut out = Vec::new();
    for v in [40u32, size, size * 2] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    for _ in 0..6 {
        out.extend_from_slice(&0u32.to_le_bytes());
    }
    for y in (0..size).rev() {
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            out.extend_from_slice(&[px[i + 2], px[i + 1], px[i], px[i + 3]]);
        }
    }
    out.extend(std::iter::repeat_n(0u8, mask_row * size as usize));
    out
}
