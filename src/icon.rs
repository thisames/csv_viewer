//! Ícone do aplicativo, desenhado por código (sem arquivos de imagem):
//! quadrado arredondado com gradiente violeta→azul e uma mini-tabela branca.

fn smoothstep(edge: f32, d: f32) -> f32 {
    // Cobertura antisserrilhada: 1 dentro, 0 fora, transição de 1 px.
    (edge - d + 0.5).clamp(0.0, 1.0)
}

/// Distância com sinal até um retângulo arredondado centrado em (cx, cy).
fn sd_round_rect(px: f32, py: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (hw - r);
    let qy = (py - cy).abs() - (hh - r);
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - r
}

fn sd_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (pax, pay) = (px - ax, py - ay);
    let (bax, bay) = (bx - ax, by - ay);
    let h = ((pax * bax + pay * bay) / (bax * bax + bay * bay)).clamp(0.0, 1.0);
    let (dx, dy) = (pax - bax * h, pay - bay * h);
    (dx * dx + dy * dy).sqrt()
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Pixels RGBA (alfa não pré-multiplicado).
pub fn render(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mut out = vec![0u8; (size * size * 4) as usize];
    let c = s / 2.0;
    let half = s * 0.44;
    let radius = s * 0.23;
    let violet = [0x9B as f32, 0x7B as f32, 0xFF as f32];
    let blue = [0x4C as f32, 0x8D as f32, 0xFF as f32];

    // Mini-tabela.
    let t_half_w = s * 0.25;
    let t_half_h = s * 0.215;
    let t_r = s * 0.06;
    let stroke = s * 0.052;
    let top = c - t_half_h;
    let bottom = c + t_half_h;
    let left = c - t_half_w;
    let right = c + t_half_w;
    let header_y = top + (bottom - top) * 0.34;
    let row_y = top + (bottom - top) * 0.67;
    let col_x = left + (right - left) * 0.42;

    for y in 0..size {
        for x in 0..size {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let d = sd_round_rect(px, py, c, c, half, half, radius);
            let a = smoothstep(0.0, d);
            if a <= 0.0 {
                continue;
            }
            let t = ((px + py) / (2.0 * s)).clamp(0.0, 1.0);
            let mut rgb = [
                lerp(violet[0], blue[0], t),
                lerp(violet[1], blue[1], t),
                lerp(violet[2], blue[2], t),
            ];
            // Brilho suave no topo.
            let glow = ((1.0 - py / s) * 0.18).max(0.0);
            for ch in &mut rgb {
                *ch = lerp(*ch, 255.0, glow);
            }

            // Faixa do cabeçalho levemente preenchida.
            let inside_table = sd_round_rect(px, py, c, c, t_half_w, t_half_h, t_r) < 0.0;
            if inside_table && py < header_y {
                for ch in &mut rgb {
                    *ch = lerp(*ch, 255.0, 0.28);
                }
            }

            let outline = sd_round_rect(px, py, c, c, t_half_w, t_half_h, t_r).abs();
            let mut white = smoothstep(stroke / 2.0, outline);
            white = white.max(smoothstep(stroke / 2.0, sd_segment(px, py, left, header_y, right, header_y)));
            white = white.max(
                smoothstep(stroke * 0.36, sd_segment(px, py, left, row_y, right, row_y)) * 0.85,
            );
            white = white.max(
                smoothstep(stroke * 0.36, sd_segment(px, py, col_x, header_y, col_x, bottom)) * 0.85,
            );
            for ch in &mut rgb {
                *ch = lerp(*ch, 255.0, white);
            }

            let i = ((y * size + x) * 4) as usize;
            out[i] = rgb[0].round() as u8;
            out[i + 1] = rgb[1].round() as u8;
            out[i + 2] = rgb[2].round() as u8;
            out[i + 3] = (a * 255.0).round() as u8;
        }
    }
    out
}

pub fn window_icon() -> egui::IconData {
    let size = 128;
    egui::IconData {
        rgba: render(size),
        width: size,
        height: size,
    }
}

/// Grava o ícone em PNG (usado pelo script de instalação).
pub fn write_png(path: &std::path::Path, size: u32) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), size, size);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer
        .write_image_data(&render(size))
        .map_err(|e| e.to_string())
}

pub fn texture(ctx: &egui::Context) -> egui::TextureHandle {
    let size = 128usize;
    let image = egui::ColorImage::from_rgba_unmultiplied([size, size], &render(size as u32));
    ctx.load_texture("logo", image, egui::TextureOptions::LINEAR)
}
