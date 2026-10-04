//! URL → 终端二维码（半块字符画 + 反色，保证扫码对比度）。

use qrcode::{Color, QrCode};

/// 渲染为多行字符串。模块用反色显示：终端深色背景下呈现"白底黑码"。
pub fn render_terminal(text: &str) -> String {
    let code = QrCode::new(text.as_bytes()).expect("二维码生成失败");
    let m = code.width();
    let quiet = 2;
    let w = m + quiet * 2;
    let colors = code.to_colors();

    let dark = |x: usize, y: usize| -> bool {
        let (mx, my) = (x as isize - quiet as isize, y as isize - quiet as isize);
        if mx < 0 || my < 0 || mx >= m as isize || my >= m as isize {
            false
        } else {
            colors[my as usize * m + mx as usize] == Color::Dark
        }
    };

    let mut out = String::new();
    let mut y = 0;
    while y < w {
        let mut line = String::from("\x1b[7m");
        for x in 0..w {
            let top = dark(x, y);
            let bot = y + 1 < w && dark(x, y + 1);
            line.push(match (top, bot) {
                (true, true) => '\u{2588}',
                (true, false) => '\u{2580}',
                (false, true) => '\u{2584}',
                (false, false) => ' ',
            });
        }
        line.push_str("\x1b[0m");
        out.push_str(&line);
        out.push('\n');
        y += 2;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::render_terminal;

    #[test]
    fn renders_square_block_of_constant_width() {
        let qr = render_terminal("http://192.168.1.5:8787/?t=ab12cd");
        let lines: Vec<&str> = qr.lines().collect();
        assert!(lines.len() >= 15, "二维码行数过少: {}", lines.len());
        let width = lines[0].chars().count();
        assert!(lines.iter().all(|l| l.chars().count() == width));
        assert!(qr.contains('\u{2588}') || qr.contains('\u{2580}'));
    }
}
