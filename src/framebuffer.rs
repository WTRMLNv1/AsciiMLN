#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalColor {
    /// An xterm-256 palette index.  This keeps materials distinct on capable terminals.
    Ansi256(u8),
    /// A true-colour value used for effects such as water and underwater tinting.
    Rgb(u8, u8, u8),
}

pub struct Framebuffer {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<char>,
    pub colors: Vec<TerminalColor>,
    pub depth: Vec<f32>,
    /// Depth written by translucent geometry.  Kept separate from `depth` so
    /// transparent surfaces do not hide the opaque scene behind them.
    transparent_depth: Vec<f32>,
}

impl Framebuffer {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![' '; width * height],
            colors: vec![TerminalColor::Ansi256(250); width * height],
            depth: vec![f32::INFINITY; width * height],
            transparent_depth: vec![f32::INFINITY; width * height],
        }
    }

    pub fn clear(&mut self) {
        for cell in self.cells.iter_mut() {
            *cell = ' ';
        }
        for depth in self.depth.iter_mut() {
            *depth = f32::INFINITY;
        }
        for depth in self.transparent_depth.iter_mut() {
            *depth = f32::INFINITY;
        }
    }

    pub fn set(&mut self, x: usize, y: usize, ch: char) {
        if x < self.width && y < self.height {
            self.cells[y * self.width + x] = ch;
        }
    }

    pub fn set_with_depth(
        &mut self,
        x: usize,
        y: usize,
        depth: f32,
        ch: char,
        color: TerminalColor,
    ) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }

        let idx = y * self.width + x;
        if depth < self.depth[idx] {
            self.depth[idx] = depth;
            self.cells[idx] = ch;
            self.colors[idx] = color;
            return true;
        }

        false
    }

    pub fn blend_transparent_with_depth(
        &mut self,
        x: usize,
        y: usize,
        depth: f32,
        ch: char,
        color: TerminalColor,
        opacity: f32,
    ) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }

        let idx = y * self.width + x;
        if depth >= self.depth[idx] || depth >= self.transparent_depth[idx] {
            return false;
        }

        self.transparent_depth[idx] = depth;
        self.colors[idx] = blend_colors(color, self.colors[idx], opacity);
        // Preserve geometry detail visible through water.  Empty cells still
        // receive the water glyph so open water has a readable surface.
        if self.cells[idx] == ' ' {
            self.cells[idx] = ch;
        }
        true
    }

    pub fn tint_blue(&mut self, amount: f32) {
        let tint = TerminalColor::Rgb(45, 122, 220);
        for color in &mut self.colors {
            *color = blend_colors(tint, *color, amount);
        }
    }

    /// Give the rendered world a moving rainbow treatment without changing its
    /// brightness, so shaded faces still have depth.
    pub fn apply_rainbow_hue(&mut self, elapsed_seconds: f32) {
        let world_height = self.height.saturating_sub(1);
        if world_height == 0 {
            return;
        }

        for y in 0..world_height {
            for x in 0..self.width {
                let index = y * self.width + x;
                if self.cells[index] != ' ' {
                    let spatial_phase =
                        x as f32 / self.width.max(1) as f32 + y as f32 / world_height as f32 * 0.15;
                    self.colors[index] =
                        rainbow_color(self.colors[index], elapsed_seconds * 0.18 + spatial_phase);
                }
            }
        }
    }

    /// Apply a mild, time-varying horizontal displacement to the world.  The
    /// final row is reserved for the status text and remains readable.
    pub fn wobble_world(&mut self, elapsed_seconds: f32) {
        let world_height = self.height.saturating_sub(1);
        if self.width == 0 || world_height == 0 {
            return;
        }

        let original_cells = self.cells.clone();
        let original_colors = self.colors.clone();
        let blank = TerminalColor::Ansi256(250);

        for y in 0..world_height {
            let wave = elapsed_seconds * 3.0 + y as f32 * 0.18;
            let shift = (wave.sin() * 1.5 + (wave * 0.43).sin() * 0.7).round() as isize;
            for x in 0..self.width {
                let destination = y * self.width + x;
                let source_x = x as isize - shift;
                if (0..self.width as isize).contains(&source_x) {
                    let source = y * self.width + source_x as usize;
                    self.cells[destination] = original_cells[source];
                    self.colors[destination] = original_colors[source];
                } else {
                    self.cells[destination] = ' ';
                    self.colors[destination] = blank;
                }
            }
        }
    }
}

fn rainbow_color(color: TerminalColor, hue_offset: f32) -> TerminalColor {
    let (r, g, b) = color.rgb();
    let (hue, saturation, value) = rgb_to_hsv(r, g, b);
    let hue = (hue + hue_offset).rem_euclid(1.0);
    // Neutral materials need some saturation too, otherwise stone and clouds
    // would not join the rainbow effect.
    hsv_to_rgb(hue, saturation.max(0.65), value.max(0.22))
}

fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / delta).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / delta + 2.0) / 6.0
    } else {
        ((r - g) / delta + 4.0) / 6.0
    };
    let saturation = if max == 0.0 { 0.0 } else { delta / max };
    (hue, saturation, max)
}

fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> TerminalColor {
    let hue = hue.rem_euclid(1.0) * 6.0;
    let chroma = value * saturation;
    let secondary = chroma * (1.0 - (hue.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match hue as u8 {
        0 => (chroma, secondary, 0.0),
        1 => (secondary, chroma, 0.0),
        2 => (0.0, chroma, secondary),
        3 => (0.0, secondary, chroma),
        4 => (secondary, 0.0, chroma),
        _ => (chroma, 0.0, secondary),
    };
    let offset = value - chroma;
    TerminalColor::Rgb(
        ((r + offset) * 255.0).round() as u8,
        ((g + offset) * 255.0).round() as u8,
        ((b + offset) * 255.0).round() as u8,
    )
}

fn blend_colors(
    foreground: TerminalColor,
    background: TerminalColor,
    opacity: f32,
) -> TerminalColor {
    let opacity = opacity.clamp(0.0, 1.0);
    let (fr, fg, fb) = foreground.rgb();
    let (br, bg, bb) = background.rgb();
    TerminalColor::Rgb(
        (fr as f32 * opacity + br as f32 * (1.0 - opacity)).round() as u8,
        (fg as f32 * opacity + bg as f32 * (1.0 - opacity)).round() as u8,
        (fb as f32 * opacity + bb as f32 * (1.0 - opacity)).round() as u8,
    )
}

impl TerminalColor {
    fn rgb(self) -> (u8, u8, u8) {
        match self {
            Self::Rgb(r, g, b) => (r, g, b),
            Self::Ansi256(index @ 16..=231) => {
                let value = index - 16;
                let component = |n| [0, 95, 135, 175, 215, 255][n as usize];
                (
                    component(value / 36),
                    component((value / 6) % 6),
                    component(value % 6),
                )
            }
            Self::Ansi256(index @ 232..=255) => {
                let gray = 8 + (index - 232) * 10;
                (gray, gray, gray)
            }
            Self::Ansi256(index) => {
                const BASIC: [(u8, u8, u8); 16] = [
                    (0, 0, 0),
                    (205, 0, 0),
                    (0, 205, 0),
                    (205, 205, 0),
                    (0, 0, 238),
                    (205, 0, 205),
                    (0, 205, 205),
                    (229, 229, 229),
                    (127, 127, 127),
                    (255, 0, 0),
                    (0, 255, 0),
                    (255, 255, 0),
                    (92, 92, 255),
                    (255, 0, 255),
                    (0, 255, 255),
                    (255, 255, 255),
                ];
                BASIC[index as usize]
            }
        }
    }
}
