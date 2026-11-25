//! Modo de desenvolvimento (feature `devshot`): executa um roteiro de
//! interações sintéticas e salva prints da própria janela em PNG.
//!
//! ```text
//! CSV_VIWER_SCRIPT="wait:20;click:400,300;key:ctrl+f;text:sao;shot:/tmp/a.png;quit" \
//!     cargo run --features devshot -- arquivo.csv
//! ```

use std::path::PathBuf;

use egui::{Event, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, TouchPhase, pos2, vec2};

enum Step {
    Wait(u32),
    Move(Pos2),
    Press(Pos2, PointerButton),
    Release(Pos2, PointerButton),
    Key(Key, Modifiers),
    KeyUp(Key, Modifiers),
    Text(String),
    Scroll(f32, f32),
    Copy,
    HoverFile(u32),
    Shot(PathBuf),
    Quit,
}

pub struct DevShot {
    steps: Vec<Step>,
    pos: usize,
    wait: u32,
    pending_shot: Option<PathBuf>,
    pointer: Pos2,
    modifiers: Modifiers,
    hover_frames: u32,
}

fn parse_point(s: &str) -> Pos2 {
    let mut it = s.split(',').map(|v| v.trim().parse::<f32>().unwrap_or(0.0));
    pos2(it.next().unwrap_or(0.0), it.next().unwrap_or(0.0))
}

fn parse_key(s: &str) -> Option<(Key, Modifiers)> {
    let mut mods = Modifiers::NONE;
    let mut key = None;
    for part in s.split('+') {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" => mods |= Modifiers::COMMAND,
            "shift" => mods |= Modifiers::SHIFT,
            "alt" => mods |= Modifiers::ALT,
            other => key = Key::from_name(part).or_else(|| Key::from_name(&other.to_uppercase())),
        }
    }
    key.map(|k| (k, mods))
}

impl DevShot {
    pub fn from_env() -> Option<Self> {
        let script = std::env::var("CSV_VIWER_SCRIPT").ok()?;
        let mut steps = Vec::new();
        for item in script.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            let (cmd, arg) = item.split_once(':').unwrap_or((item, ""));
            match cmd {
                "wait" => steps.push(Step::Wait(arg.parse().unwrap_or(10))),
                "move" => steps.push(Step::Move(parse_point(arg))),
                "click" | "rclick" | "dclick" => {
                    let p = parse_point(arg);
                    let b = if cmd == "rclick" { PointerButton::Secondary } else { PointerButton::Primary };
                    steps.push(Step::Move(p));
                    steps.push(Step::Wait(2));
                    let times = if cmd == "dclick" { 2 } else { 1 };
                    for _ in 0..times {
                        steps.push(Step::Press(p, b));
                        steps.push(Step::Release(p, b));
                    }
                    steps.push(Step::Wait(3));
                }
                "drag" => {
                    let mut pts = arg.split("->");
                    let a = parse_point(pts.next().unwrap_or("0,0"));
                    let b = parse_point(pts.next().unwrap_or("0,0"));
                    steps.push(Step::Move(a));
                    steps.push(Step::Press(a, PointerButton::Primary));
                    for i in 1..=8 {
                        let t = i as f32 / 8.0;
                        steps.push(Step::Move(a + (b - a) * t));
                    }
                    steps.push(Step::Release(b, PointerButton::Primary));
                    steps.push(Step::Wait(3));
                }
                "key" => {
                    if let Some((k, m)) = parse_key(arg) {
                        steps.push(Step::Key(k, m));
                        steps.push(Step::KeyUp(k, m));
                        steps.push(Step::Wait(2));
                    } else {
                        eprintln!("devshot: tecla desconhecida {arg}");
                    }
                }
                "text" => {
                    steps.push(Step::Text(arg.to_owned()));
                    steps.push(Step::Wait(2));
                }
                "scroll" => {
                    let p = parse_point(arg);
                    steps.push(Step::Scroll(p.x, p.y));
                    steps.push(Step::Wait(4));
                }
                "copy" => steps.push(Step::Copy),
                "hoverfile" => steps.push(Step::HoverFile(arg.parse().unwrap_or(30))),
                "shot" => steps.push(Step::Shot(PathBuf::from(arg))),
                "quit" => steps.push(Step::Quit),
                other => eprintln!("devshot: comando desconhecido {other}"),
            }
        }
        Some(Self {
            steps,
            pos: 0,
            wait: 25,
            pending_shot: None,
            pointer: pos2(-100.0, -100.0),
            modifiers: Modifiers::NONE,
            hover_frames: 0,
        })
    }

    /// Injeta eventos do roteiro na entrada do quadro.
    pub fn hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        ctx.request_repaint();
        if self.hover_frames > 0 {
            self.hover_frames -= 1;
            raw.hovered_files.push(egui::HoveredFile {
                path: Some(PathBuf::from("planilha.csv")),
                mime: String::new(),
            });
        }
        if self.pending_shot.is_some() {
            return;
        }
        if self.wait > 0 {
            self.wait -= 1;
            raw.events.push(Event::PointerMoved(self.pointer));
            return;
        }
        // Um passo por quadro.
        if let Some(step) = self.steps.get(self.pos) {
            self.pos += 1;
            match step {
                Step::Wait(n) => {
                    self.wait = *n;
                }
                Step::Move(p) => {
                    self.pointer = *p;
                    raw.events.push(Event::PointerMoved(*p));
                }
                Step::Press(p, b) => {
                    raw.events.push(Event::PointerButton {
                        pos: *p,
                        button: *b,
                        pressed: true,
                        modifiers: self.modifiers,
                    });
                }
                Step::Release(p, b) => {
                    raw.events.push(Event::PointerButton {
                        pos: *p,
                        button: *b,
                        pressed: false,
                        modifiers: self.modifiers,
                    });
                }
                Step::Key(k, m) => {
                    self.modifiers = *m;
                    raw.events.push(Event::ModifiersChanged(*m));
                    raw.events.push(Event::Key {
                        key: *k,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: *m,
                    });
                    if m.command && *k == Key::C {
                        raw.events.push(Event::Copy);
                    }
                }
                Step::KeyUp(k, m) => {
                    raw.events.push(Event::Key {
                        key: *k,
                        physical_key: None,
                        pressed: false,
                        repeat: false,
                        modifiers: *m,
                    });
                    self.modifiers = Modifiers::NONE;
                    raw.events.push(Event::ModifiersChanged(Modifiers::NONE));
                }
                Step::Text(t) => {
                    raw.events.push(Event::Text(t.clone()));
                }
                Step::Scroll(dx, dy) => {
                    raw.events.push(Event::PointerMoved(self.pointer));
                    raw.events.push(Event::MouseWheel {
                        unit: MouseWheelUnit::Point,
                        delta: vec2(*dx, *dy),
                        phase: TouchPhase::Move,
                        modifiers: self.modifiers,
                    });
                }
                Step::HoverFile(n) => {
                    self.hover_frames = *n;
                }
                Step::Copy => {
                    raw.events.push(Event::Copy);
                }
                Step::Shot(path) => {
                    self.pending_shot = Some(path.clone());
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                }
                Step::Quit => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    /// Recebe o print pedido e grava em PNG.
    pub fn receive(&mut self, ctx: &egui::Context) {
        let Some(path) = self.pending_shot.clone() else {
            return;
        };
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            if let Err(e) = save_png(&path, &image) {
                eprintln!("devshot: erro ao salvar {}: {e}", path.display());
            } else {
                eprintln!("devshot: salvo {}", path.display());
            }
            self.pending_shot = None;
            self.wait = 2;
        }
    }
}

fn save_png(path: &std::path::Path, image: &egui::ColorImage) -> Result<(), String> {
    let [w, h] = image.size;
    let mut data = Vec::with_capacity(w * h * 4);
    for px in &image.pixels {
        let [r, g, b, a] = px.to_srgba_unmultiplied();
        data.extend_from_slice(&[r, g, b, a]);
    }
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&data).map_err(|e| e.to_string())?;
    Ok(())
}
