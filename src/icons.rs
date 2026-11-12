//! Tray icons drawn at runtime so they stay crisp at any size and follow the
//! taskbar or menu bar colour without shipping image files.

use crate::engine::Mode;

pub const RED: [u8; 3] = [0xFF, 0x3B, 0x30];
pub const ACCENT: [u8; 3] = [0x0A, 0x84, 0xFF];

fn ring(x: f32, y: f32, r: f32, w: f32) -> bool {
    let d = (x * x + y * y).sqrt();
    (d - r).abs() <= w / 2.0
}

fn disc(x: f32, y: f32, r: f32) -> bool {
    x * x + y * y <= r * r
}

fn play(x: f32, y: f32, s: f32) -> bool {
    // Right-pointing triangle centred optically (shifted right a little).
    let x = x + s * 0.12;
    let h = s * 0.95;
    x >= -s * 0.5 && x <= s * 0.62 && y.abs() <= h * 0.5 * (s * 0.62 - x) / (s * 1.12)
}

/// Coverage of the glyph at a point in [-0.5, 0.5] square coordinates.
/// Second value marks pixels drawn in the status colour.
fn shape(mode: Mode, x: f32, y: f32) -> (bool, bool) {
    match mode {
        Mode::Idle => (ring(x, y, 0.36, 0.085) || disc(x, y, 0.14), false),
        Mode::Recording => {
            let dot = disc(x, y, 0.22);
            (ring(x, y, 0.38, 0.085) || dot, dot)
        }
        Mode::Playing => {
            let tri = play(x, y, 0.34);
            (ring(x, y, 0.38, 0.085) || tri, tri)
        }
    }
}

/// RGBA pixels for a tray icon. `fg` is the glyph colour; when `colored` is
/// set the inner mark uses the status colour (red while recording, blue
/// while playing).
pub fn tray(mode: Mode, size: u32, fg: [u8; 3], colored: bool) -> Vec<u8> {
    const SS: u32 = 4;
    let mut out = vec![0u8; (size * size * 4) as usize];
    let status = if mode == Mode::Recording { RED } else { ACCENT };
    for py in 0..size {
        for px in 0..size {
            let mut cov = 0u32;
            let mut tint = 0u32;
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = (px as f32 + (sx as f32 + 0.5) / SS as f32) / size as f32 - 0.5;
                    let y = (py as f32 + (sy as f32 + 0.5) / SS as f32) / size as f32 - 0.5;
                    let (on, inner) = shape(mode, x, y);
                    if on {
                        cov += 1;
                        if inner {
                            tint += 1;
                        }
                    }
                }
            }
            if cov == 0 {
                continue;
            }
            let i = ((py * size + px) * 4) as usize;
            let c = if colored && tint * 2 >= cov {
                status
            } else {
                fg
            };
            out[i..i + 3].copy_from_slice(&c);
            out[i + 3] = (cov * 255 / (SS * SS)) as u8;
        }
    }
    out
}
