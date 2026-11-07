use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use calloop::channel::Channel;
use calloop::EventLoop;
use calloop_wayland_source::WaylandSource;
use fontdue::{Font, FontSettings};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, PixmapMut, Rect, Transform};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_shm, wl_surface},
    Connection, QueueHandle,
};

use crate::config::AppConfig;
use crate::input::GuiMessage;
use crate::parser::KeybindRegistry;

pub struct OutputSurface {
    pub output: wl_output::WlOutput,
    pub name: Option<String>,
    pub surface: wl_surface::WlSurface,
    pub layer: LayerSurface,
    pub width: u32,
    pub height: u32,
    pub configured: bool,
    pub is_showing: bool,
}

pub struct GuiApp {
    registry_state: RegistryState,
    output_state: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    pool: SlotPool,
    #[allow(dead_code)]
    qh: QueueHandle<GuiApp>,

    output_surfaces: Vec<OutputSurface>,
    focused_output_name: Option<String>,

    is_visible: bool,
    active_chord: Option<BTreeSet<String>>,

    keybinds: KeybindRegistry,
    config: AppConfig,
    font: Font,
}

impl CompositorHandler for GuiApp {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for GuiApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.create_surface_for_output(output, qh);
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if let Some(info) = self.output_state.info(&output) {
            if let Some(surf) = self.output_surfaces.iter_mut().find(|s| s.output == output) {
                surf.name = info.name;
            }
        }
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.output_surfaces.retain(|s| s.output != output);
    }
}

impl LayerShellHandler for GuiApp {
    fn closed(&mut self, _conn: &Connection, qh: &QueueHandle<Self>, layer: &LayerSurface) {
        if let Some(idx) = self
            .output_surfaces
            .iter()
            .position(|s| s.layer.wl_surface() == layer.wl_surface())
        {
            let output = self.output_surfaces[idx].output.clone();
            self.output_surfaces.remove(idx);
            self.create_surface_for_output(output, qh);
        }
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let mut needs_empty_draw = false;
        if let Some(surf) = self
            .output_surfaces
            .iter_mut()
            .find(|s| s.layer.wl_surface() == layer.wl_surface())
        {
            surf.configured = true;
            if configure.new_size.0 > 0 {
                surf.width = configure.new_size.0;
            }
            if configure.new_size.1 > 0 {
                surf.height = configure.new_size.1;
            }
            if !surf.is_showing {
                needs_empty_draw = true;
            }
        }

        if needs_empty_draw {
            if let Some(surf) = self
                .output_surfaces
                .iter_mut()
                .find(|s| s.layer.wl_surface() == layer.wl_surface())
            {
                Self::draw_surface_empty(&mut self.pool, surf);
            }
        }

        if self.is_visible {
            self.draw();
        }
    }
}

impl ShmHandler for GuiApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for GuiApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

delegate_registry!(GuiApp);
smithay_client_toolkit::delegate_dispatch2!(GuiApp);

pub fn run_gui(config: AppConfig, channel_source: Channel<GuiMessage>) {
    let conn = match Connection::connect_to_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "[niri-whichkey] Failed to connect to Wayland compositor: {}",
                e
            );
            return;
        }
    };

    let (globals, event_queue) =
        registry_queue_init(&conn).expect("Failed to init Wayland registry queue");
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor is not available");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("layer shell is not available");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm is not available");
    let output_state = OutputState::new(&globals, &qh);

    let pool = SlotPool::new(1920 * 1080 * 4, &shm).expect("Failed to create SHM slot pool");
    let keybinds = KeybindRegistry::load_from(&config.niri_config_path);
    let font = load_font();

    let mut app = GuiApp {
        registry_state: RegistryState::new(&globals),
        output_state,
        compositor,
        layer_shell,
        shm,
        pool,
        qh: qh.clone(),
        output_surfaces: Vec::new(),
        focused_output_name: None,
        is_visible: false,
        active_chord: None,
        keybinds,
        config,
        font,
    };

    // Pre-create surfaces for currently connected outputs
    let initial_outputs: Vec<wl_output::WlOutput> = app.output_state.outputs().collect();
    for output in initial_outputs {
        app.create_surface_for_output(output, &qh);
    }

    let mut event_loop: EventLoop<GuiApp> =
        EventLoop::try_new().expect("Failed to create calloop event loop");
    let loop_handle = event_loop.handle();

    // Insert Wayland source into calloop
    WaylandSource::new(conn, event_queue)
        .insert(loop_handle.clone())
        .expect("Failed to insert Wayland source into calloop");

    // Insert MPSC channel from evdev & IPC into calloop
    loop_handle
        .insert_source(channel_source, |event, _, state: &mut GuiApp| {
            if let calloop::channel::Event::Msg(msg) = event {
                state.handle_gui_message(msg);
            }
        })
        .expect("Failed to insert channel source into calloop");

    while event_loop.dispatch(None, &mut app).is_ok() {
        // Continue running event loop
    }
}

impl GuiApp {
    fn create_surface_for_output(&mut self, output: wl_output::WlOutput, qh: &QueueHandle<Self>) {
        if self.output_surfaces.iter().any(|s| s.output == output) {
            return;
        }

        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface.clone(),
            Layer::Top,
            Some("niri-whichkey"),
            Some(&output),
        );

        layer.set_anchor(Anchor::TOP | Anchor::RIGHT);
        layer.set_margin(self.config.margin_top, self.config.margin_right, 0, 0);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_size(1, 1);
        layer.commit();

        let name = self.output_state.info(&output).and_then(|info| info.name);

        self.output_surfaces.push(OutputSurface {
            output,
            name,
            surface,
            layer,
            width: 1,
            height: 1,
            configured: false,
            is_showing: false,
        });
    }

    fn draw_surface_empty(pool: &mut SlotPool, surf: &mut OutputSurface) {
        surf.width = 1;
        surf.height = 1;
        surf.layer.set_size(1, 1);
        let (buffer, _canvas) = match pool.create_buffer(1, 1, 4, wl_shm::Format::Argb8888) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("[niri-whichkey] Failed to create empty SHM buffer: {:?}", e);
                return;
            }
        };
        surf.surface.damage_buffer(0, 0, 1, 1);
        if let Err(e) = buffer.attach_to(&surf.surface) {
            eprintln!("[niri-whichkey] Failed to attach empty buffer: {:?}", e);
            return;
        }
        surf.layer.commit();
        surf.is_showing = false;
    }

    pub fn handle_gui_message(&mut self, msg: GuiMessage) {
        match msg {
            GuiMessage::FocusedOutput(name) => {
                let changed = self.focused_output_name.as_deref() != Some(&name);
                self.focused_output_name = Some(name);
                if changed && self.is_visible {
                    self.draw();
                }
            }
            GuiMessage::Show(chord) => {
                self.active_chord = Some(chord);
                self.is_visible = true;
                self.draw();
            }
            GuiMessage::Hide => {
                if self.is_visible {
                    self.is_visible = false;
                    self.active_chord = None;
                    self.draw_empty();
                }
            }
            GuiMessage::ReloadConfig => {
                self.keybinds = KeybindRegistry::load_from(&self.config.niri_config_path);
                if self.is_visible {
                    self.draw();
                }
            }
        }
    }

    fn draw(&mut self) {
        if !self.is_visible {
            return;
        }

        let chord = match &self.active_chord {
            Some(c) => c.clone(),
            None => return,
        };

        if self.output_surfaces.is_empty() {
            return;
        }

        let (direct, sub_chords) = self.keybinds.lookup(&chord);
        let items_count = direct.len();

        let use_two_cols = items_count > 8 && self.config.max_columns > 1;
        let col_width = 320u32;
        let padding = 16u32;
        let header_height = 42u32;
        let row_height = 24u32;

        let num_rows = if use_two_cols {
            (items_count as u32).div_ceil(2)
        } else {
            items_count as u32
        };

        let items_height = num_rows * row_height;
        let sub_chords_height = if !sub_chords.is_empty() { 36u32 } else { 0u32 };

        let calculated_width = if use_two_cols {
            col_width * 2 + padding * 3
        } else {
            col_width + padding * 2
        };

        let calculated_height = padding * 2 + header_height + items_height + sub_chords_height;

        let width = calculated_width.max(280);
        let height = calculated_height.max(80);
        let stride = width as i32 * 4;

        let target_idx = if let Some(ref focused_name) = self.focused_output_name {
            self.output_surfaces
                .iter()
                .position(|s| s.name.as_deref() == Some(focused_name.as_str()))
                .unwrap_or(0)
        } else {
            0
        };

        for i in 0..self.output_surfaces.len() {
            if i == target_idx {
                let surf = &mut self.output_surfaces[i];
                surf.width = width;
                surf.height = height;
                surf.layer.set_size(width, height);

                let (buffer, canvas) = match self.pool.create_buffer(
                    width as i32,
                    height as i32,
                    stride,
                    wl_shm::Format::Argb8888,
                ) {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("[niri-whichkey] Failed to create SHM buffer: {:?}", e);
                        continue;
                    }
                };

                render_card(
                    canvas,
                    width,
                    height,
                    &chord,
                    &self.keybinds,
                    &self.font,
                    self.config.font_size,
                    self.config.max_columns,
                );

                surf.surface
                    .damage_buffer(0, 0, width as i32, height as i32);
                if let Err(e) = buffer.attach_to(&surf.surface) {
                    eprintln!("[niri-whichkey] Failed to attach buffer: {:?}", e);
                    continue;
                }
                surf.layer.commit();
                surf.is_showing = true;
            } else {
                let surf = &mut self.output_surfaces[i];
                if surf.is_showing {
                    Self::draw_surface_empty(&mut self.pool, surf);
                }
            }
        }
    }

    fn draw_empty(&mut self) {
        for surf in &mut self.output_surfaces {
            Self::draw_surface_empty(&mut self.pool, surf);
        }
    }
}

fn load_font() -> Font {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let mut font_candidates = vec![
        format!("{}/.nix-profile/share/fonts/truetype/NerdFonts/JetBrainsMono/JetBrainsMonoNLNerdFontMono-Regular.ttf", home),
        format!("{}/.local/share/fonts/JetBrainsMono-Regular.ttf", home),
        "/run/current-system/sw/share/fonts/truetype/JetBrainsMonoNerdFont-Regular.ttf".to_string(),
        "/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Regular.ttf".to_string(),
        "/usr/share/fonts/TTF/DejaVuSans.ttf".to_string(),
        "/usr/share/fonts/TTF/FiraSans-Regular.ttf".to_string(),
        "/usr/share/fonts/noto/NotoSans-Regular.ttf".to_string(),
    ];

    if let Ok(entries) = std::fs::read_dir("/usr/share/fonts/TTF") {
        for entry in entries.flatten() {
            if entry.path().extension().and_then(|s| s.to_str()) == Some("ttf") {
                font_candidates.push(entry.path().to_string_lossy().to_string());
            }
        }
    }

    for path in &font_candidates {
        if let Ok(mut f) = File::open(Path::new(path)) {
            let mut buf = Vec::new();
            if f.read_to_end(&mut buf).is_ok() {
                if let Ok(font) = Font::from_bytes(buf, FontSettings::default()) {
                    return font;
                }
            }
        }
    }

    panic!(
        "[niri-whichkey] Could not find any suitable TTF font on system! Checked candidates: {:?}",
        font_candidates
    );
}

#[allow(clippy::too_many_arguments)]
fn render_card(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    active_chord: &BTreeSet<String>,
    keybinds: &KeybindRegistry,
    font: &Font,
    font_size: f32,
    max_columns: usize,
) {
    let mut pixmap = match PixmapMut::from_bytes(canvas, width, height) {
        Some(p) => p,
        None => return,
    };

    pixmap.fill(Color::TRANSPARENT);

    let (direct_binds, sub_chords) = keybinds.lookup(active_chord);

    let w = width as f32;
    let h = height as f32;
    let radius = 12.0;

    // 1. Draw Background Card (Obsidian Translucent #181825f0)
    let bg_path = rounded_rect_path(0.0, 0.0, w, h, radius);
    let mut bg_paint = Paint::default();
    bg_paint.set_color_rgba8(24, 24, 37, 240);
    pixmap.fill_path(
        &bg_path,
        &bg_paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );

    // 2. Draw Subtle Card Border (#45475a)
    let mut border_paint = Paint::default();
    border_paint.set_color_rgba8(69, 71, 90, 220);
    let stroke = tiny_skia::Stroke {
        width: 1.5,
        ..Default::default()
    };
    pixmap.stroke_path(
        &bg_path,
        &border_paint,
        &stroke,
        Transform::identity(),
        None,
    );

    // 3. Draw Header
    let pad_x = 16.0;
    let mut cur_y = 16.0;

    let chord_title = if active_chord.is_empty() {
        "Niri Hotkeys".to_string()
    } else {
        format!(
            "Niri Hotkeys ({})",
            active_chord.iter().cloned().collect::<Vec<_>>().join(" + ")
        )
    };

    draw_text(
        &mut pixmap,
        font,
        &chord_title,
        pad_x,
        cur_y + 14.0,
        font_size + 2.0,
        Color::from_rgba8(205, 214, 244, 255),
    );

    let count_text = format!("{} available", direct_binds.len());
    let count_x = w - pad_x - (count_text.len() as f32 * 7.5);
    draw_text(
        &mut pixmap,
        font,
        &count_text,
        count_x,
        cur_y + 13.0,
        font_size - 1.0,
        Color::from_rgba8(166, 173, 200, 255),
    );

    cur_y += 28.0;

    // Header divider line
    let mut divider_paint = Paint::default();
    divider_paint.set_color_rgba8(49, 50, 68, 255);
    if let Some(rect) = Rect::from_xywh(pad_x, cur_y, w - pad_x * 2.0, 1.0) {
        pixmap.fill_rect(rect, &divider_paint, Transform::identity(), None);
    }
    cur_y += 10.0;

    // 4. Draw Keybindings Grid
    let use_two_cols = direct_binds.len() > 8 && max_columns > 1;
    let col_w = if use_two_cols {
        (w - pad_x * 3.0) / 2.0
    } else {
        w - pad_x * 2.0
    };
    let row_h = 24.0;

    for (i, bind) in direct_binds.iter().enumerate() {
        let (col, row) = if use_two_cols { (i % 2, i / 2) } else { (0, i) };

        let item_x = pad_x + (col as f32) * (col_w + pad_x);
        let item_y = cur_y + (row as f32) * row_h;

        // Key badge with dynamic measurement and prettification
        let display_key = prettify_key(&bind.key);
        let key_text_w = text_width(font, &display_key, font_size - 1.0);
        let badge_w = (key_text_w + 12.0).max(24.0);
        let badge_path = rounded_rect_path(item_x, item_y, badge_w, 18.0, 4.0);

        let mut badge_bg = Paint::default();
        badge_bg.set_color_rgba8(49, 50, 68, 255);
        pixmap.fill_path(
            &badge_path,
            &badge_bg,
            FillRule::Winding,
            Transform::identity(),
            None,
        );

        let badge_text_x = item_x + (badge_w - key_text_w) / 2.0;
        draw_text(
            &mut pixmap,
            font,
            &display_key,
            badge_text_x,
            item_y + 13.0,
            font_size - 1.0,
            Color::from_rgba8(137, 180, 250, 255),
        );

        // Description text
        let desc_x = item_x + badge_w + 8.0;
        let max_desc_chars = ((col_w - badge_w - 12.0) / 7.5).max(10.0) as usize;
        let mut display_title = bind.title.clone();
        if display_title.len() > max_desc_chars && max_desc_chars > 3 {
            display_title.truncate(max_desc_chars - 3);
            display_title.push_str("...");
        }

        draw_text(
            &mut pixmap,
            font,
            &display_title,
            desc_x,
            item_y + 13.0,
            font_size,
            Color::from_rgba8(186, 194, 222, 255),
        );
    }

    // 5. Draw Sub-chords footer if any exist
    if !sub_chords.is_empty() {
        let num_rows = if use_two_cols {
            direct_binds.len().div_ceil(2)
        } else {
            direct_binds.len()
        };
        let footer_y = cur_y + (num_rows as f32) * row_h + 4.0;

        if let Some(rect) = Rect::from_xywh(pad_x, footer_y, w - pad_x * 2.0, 1.0) {
            pixmap.fill_rect(rect, &divider_paint, Transform::identity(), None);
        }

        let mut sub_x = pad_x;
        draw_text(
            &mut pixmap,
            font,
            "Extend: ",
            sub_x,
            footer_y + 18.0,
            font_size - 1.0,
            Color::from_rgba8(147, 153, 178, 255),
        );
        sub_x += 52.0;

        for hint in sub_chords {
            let label = format!("+{} ({}) ❯ ", hint.next_modifier, hint.count);
            let len_px = label.len() as f32 * 7.5;
            if sub_x + len_px < w - pad_x {
                draw_text(
                    &mut pixmap,
                    font,
                    &label,
                    sub_x,
                    footer_y + 18.0,
                    font_size - 1.0,
                    Color::from_rgba8(249, 226, 175, 255),
                );
                sub_x += len_px + 4.0;
            }
        }
    }
}

fn prettify_key(key: &str) -> String {
    match key {
        "Left" => "←".to_string(),
        "Right" => "→".to_string(),
        "Up" => "↑".to_string(),
        "Down" => "↓".to_string(),
        "Return" | "Enter" => "↵".to_string(),
        "BackSpace" => "⌫".to_string(),
        "Tab" => "⇥".to_string(),
        "Escape" => "Esc".to_string(),
        "Space" => "␣".to_string(),
        "Minus" => "-".to_string(),
        "Equal" => "=".to_string(),
        "BracketLeft" => "[".to_string(),
        "BracketRight" => "]".to_string(),
        "Semicolon" => ";".to_string(),
        "Apostrophe" => "'".to_string(),
        "Grave" => "`".to_string(),
        "Backslash" => "\\".to_string(),
        "Comma" => ",".to_string(),
        "Period" => ".".to_string(),
        "Slash" => "/".to_string(),
        other => other.to_string(),
    }
}

fn rounded_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> tiny_skia::Path {
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
        .unwrap_or_else(|| PathBuilder::new().finish().unwrap())
}

fn text_width(font: &Font, text: &str, size: f32) -> f32 {
    let mut w = 0.0;
    for ch in text.chars() {
        let (metrics, _) = font.rasterize(ch, size);
        w += metrics.advance_width;
    }
    w
}

fn draw_text(
    pixmap: &mut PixmapMut,
    font: &Font,
    text: &str,
    mut x: f32,
    y: f32,
    size: f32,
    color: Color,
) {
    let max_w = pixmap.width() as f32;
    let max_h = pixmap.height() as f32;

    for ch in text.chars() {
        let (metrics, bitmap) = font.rasterize(ch, size);
        let gx = (x + metrics.xmin as f32).round() as i32;
        let gy = (y - metrics.height as f32 - metrics.ymin as f32).round() as i32;

        if gx >= 0 && gy >= 0 {
            for row in 0..metrics.height {
                for col in 0..metrics.width {
                    let alpha_val = bitmap[row * metrics.width + col];
                    if alpha_val == 0 {
                        continue;
                    }
                    let px = gx + col as i32;
                    let py = gy + row as i32;

                    if px >= 0 && px < max_w as i32 && py >= 0 && py < max_h as i32 {
                        let factor = (alpha_val as f32 / 255.0) * color.alpha();
                        blend_pixel(pixmap, px as u32, py as u32, color, factor);
                    }
                }
            }
        }
        x += metrics.advance_width;
    }
}

fn blend_pixel(pixmap: &mut PixmapMut, x: u32, y: u32, color: Color, factor: f32) {
    let idx = ((y * pixmap.width() + x) * 4) as usize;
    let bytes = pixmap.data_mut();

    let src_r = (color.red() * 255.0) as u32;
    let src_g = (color.green() * 255.0) as u32;
    let src_b = (color.blue() * 255.0) as u32;
    let src_a = (factor * 255.0).min(255.0) as u32;

    let dst_b = bytes[idx] as u32;
    let dst_g = bytes[idx + 1] as u32;
    let dst_r = bytes[idx + 2] as u32;
    let dst_a = bytes[idx + 3] as u32;

    let out_a = src_a + (dst_a * (255 - src_a)) / 255;
    if out_a == 0 {
        return;
    }

    let out_r = (src_r * src_a + dst_r * (255 - src_a)) / 255;
    let out_g = (src_g * src_a + dst_g * (255 - src_a)) / 255;
    let out_b = (src_b * src_a + dst_b * (255 - src_a)) / 255;

    bytes[idx] = out_b as u8;
    bytes[idx + 1] = out_g as u8;
    bytes[idx + 2] = out_r as u8;
    bytes[idx + 3] = out_a as u8;
}
