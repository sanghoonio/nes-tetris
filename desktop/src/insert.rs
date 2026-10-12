//! INSERT ROM: the screen on first launch, before there's a ROM, as the web page's: the settings
//! panel's dotted yellow frame with INSERT ROM and, dimmed, NES TETRIS (USA). Drop the file on the
//! window, or click (or Enter, or a pad's bottom button) for the Open dialog. The game's own font is
//! in the ROM, so this is drawn in Brickface, the web page's (`web/font/glyphs.txt`).

use std::path::Path;
use std::sync::Arc;

use tetris_rs::Rom;

const WIDTH: usize = 256;
const HEIGHT: usize = 240;

/// NES palette indices: the web page's colours.
const BLACK: u8 = 0x0F;
const WHITE: u8 = 0x30;
const DIM: u8 = 0x10;
const YELLOW: u8 = 0x38;
const BLUE: u8 = 0x3C;
const RED: u8 = 0x26;

/// The panel as on the web page (in the font's pixels): 20 above and below the text and 24 each
/// side, lines 12 high, the frame 4 in from the edge.
const PAD_Y: usize = 20;
const PAD_X: usize = 24;
const LINE: usize = 12;
const INSET: usize = 4;

pub struct Insert {
    /// The last file wasn't Tetris (USA).
    refused: bool,
}

/// What a frame of INSERT ROM came to.
pub enum Outcome {
    Waiting,
    /// A ROM that runs, and its file (to keep).
    Rom(Arc<Rom>, Vec<u8>),
    Quit,
}

impl Insert {
    pub fn new() -> Self {
        Insert { refused: false }
    }

    /// Take a dropped file, or open the dialog on a click or Enter; Escape quits.
    pub fn frame(&mut self, input: &frontend::Input) -> (Vec<u8>, Outcome) {
        let mut outcome = Outcome::Waiting;
        let chosen = input.dropped.last().cloned().or_else(|| {
            let ask = input.clicked || input.pressed.iter().any(|k| matches!(k.as_str(), "Enter" | "NumpadEnter" | "Space" | "PadSouth" | "PadStart"));
            ask.then(|| rfd::FileDialog::new().set_title("Insert ROM").add_filter("NES ROM", &["nes"]).pick_file()).flatten()
        });
        if let Some(path) = chosen {
            match read(&path) {
                Some((rom, file)) => outcome = Outcome::Rom(rom, file),
                None => self.refused = true,
            }
        } else if input.pressed.iter().any(|k| k == "Escape" || k == "PadEast") {
            outcome = Outcome::Quit;
        }
        (self.draw(input.hovering), outcome)
    }

    fn draw(&self, hovering: bool) -> Vec<u8> {
        let mut pixels = vec![BLACK; WIDTH * HEIGHT];
        // Lines and the gap above each; the panel fits them, as the web page's does.
        let mut lines: Vec<(&str, u8, usize)> = vec![("INSERT ROM", WHITE, 0), ("NES TETRIS (USA)", DIM, 0)];
        if self.refused {
            lines.push(("THIS ROM WON'T RUN.", RED, LINE));
        }
        let longest = lines.iter().map(|(t, ..)| t.len()).max().unwrap_or(0) * 8;
        let tall: usize = lines.iter().map(|(.., gap)| gap + LINE).sum();
        let (w, h) = (longest + 2 * PAD_X, tall + 2 * PAD_Y);
        let (x0, y0) = ((WIDTH - w) / 2, (HEIGHT - h) / 2);
        frame(&mut pixels, x0 + INSET, y0 + INSET, w - 2 * INSET, h - 2 * INSET, if hovering { BLUE } else { YELLOW });
        let mut y = y0 + PAD_Y;
        for (text, colour, gap) in lines {
            y += gap;
            text_centred(&mut pixels, y, text, colour);
            y += LINE;
        }
        pixels
    }
}

/// The file at `path`, if it's Tetris (USA).
fn read(path: &Path) -> Option<(Arc<Rom>, Vec<u8>)> {
    let file = std::fs::read(path).map_err(|e| eprintln!("{}: {e}", path.display())).ok()?;
    match Rom::new(&file) {
        Ok(rom) => Some((Arc::new(rom), file)),
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            None
        }
    }
}

/// The settings panel's frame: a dotted line two pixels thick, solid at the corners (3 by 2).
fn frame(pixels: &mut [u8], x: usize, y: usize, w: usize, h: usize, colour: u8) {
    let mut set = |px: usize, py: usize| pixels[py * WIDTH + px] = colour;
    for dx in (0..w).step_by(2) {
        for dy in [0, 1, h - 2, h - 1] {
            set(x + dx, y + dy);
        }
    }
    for dy in (0..h).step_by(2) {
        for dx in [0, 1, w - 2, w - 1] {
            set(x + dx, y + dy);
        }
    }
    for (cx, cy) in [(x, y), (x + w - 3, y), (x, y + h - 2), (x + w - 3, y + h - 2)] {
        for dy in 0..2 {
            for dx in 0..3 {
                set(cx + dx, cy + dy);
            }
        }
    }
}

/// `text` centred across the screen in a line 12 high from `y` (the 8-high glyphs in its middle).
fn text_centred(pixels: &mut [u8], y: usize, text: &str, colour: u8) {
    let x = (WIDTH - 8 * text.chars().count()) / 2;
    for (i, c) in text.chars().enumerate() {
        let Some(rows) = glyph(c) else { continue };
        for (dy, row) in rows.iter().enumerate() {
            for dx in 0..7 {
                if row >> (6 - dx) & 1 != 0 {
                    pixels[(y + 2 + dy) * WIDTH + x + 8 * i + dx] = colour;
                }
            }
        }
    }
}

/// Brickface's 7 by 7 rows for `c` (a bit per pixel, leftmost highest), if it has one.
fn glyph(c: char) -> Option<[u8; 7]> {
    const SOURCE: &str = include_str!("../../web/font/glyphs.txt");
    let mut lines = SOURCE.lines().filter(|l| !l.is_empty() && !l.starts_with("# "));
    while let Some(header) = lines.next() {
        let rows: Vec<&str> = lines.by_ref().take(7).collect();
        if header.chars().any(|h| h == c.to_ascii_uppercase()) {
            let mut glyph = [0u8; 7];
            for (r, row) in rows.iter().enumerate() {
                glyph[r] = row.chars().fold(0, |bits, p| bits << 1 | (p == '#') as u8);
            }
            return Some(glyph);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_letter_it_shows() {
        for c in "INSERT ROM NES TETRIS (USA) THIS ROM WON'T RUN.".chars().filter(|&c| c != ' ') {
            assert!(glyph(c).is_some(), "no glyph for {c:?}");
        }
        // A: the top row is ..###..
        assert_eq!(glyph('A').unwrap()[0], 0b0011100);
    }

    /// Save the screen as `target/desktop/<name>.png` (to look at).
    fn shot(pixels: &[u8], name: &str) {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/desktop");
        std::fs::create_dir_all(&dir).unwrap();
        let mut ppm = format!("P6 {WIDTH} {HEIGHT} 255\n").into_bytes();
        for &p in pixels {
            let rgb = frontend::PALETTE[(p & 0x3F) as usize];
            ppm.extend_from_slice(&[(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8]);
        }
        let ppm_path = dir.join(format!("{name}.ppm"));
        std::fs::write(&ppm_path, ppm).unwrap();
        let png = dir.join(format!("{name}.png"));
        let converted = std::process::Command::new("sips").args(["-s", "format", "png"]).arg(&ppm_path).arg("--out").arg(&png).output();
        if converted.is_ok_and(|o| o.status.success()) {
            let _ = std::fs::remove_file(ppm_path);
        }
    }

    #[test]
    fn the_screen() {
        let mut insert = Insert::new();
        shot(&insert.draw(false), "insert_rom");
        shot(&insert.draw(true), "insert_rom_hovering");
        // Not Tetris: refused, and the panel widens for the message.
        let path = std::env::temp_dir().join("not-tetris.nes");
        std::fs::write(&path, b"NES\x1A not tetris").unwrap();
        let input = frontend::Input { dropped: vec![path], ..Default::default() };
        let (picture, outcome) = insert.frame(&input);
        assert!(matches!(outcome, Outcome::Waiting));
        shot(&picture, "insert_rom_refused");
    }
}
