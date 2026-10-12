//! Frames: the NMI handler, the vblank waits, and running the game a frame at a time.
//!
//! The original runs its game logic in a main loop and does its per-frame video work in the NMI
//! handler, which the PPU triggers at the start of vblank. The game only ever idles in two wait
//! loops when that happens (`recomp/tests/nmi_sites.rs` checks this), so here the waits call the
//! handler themselves: a frame ends exactly where the original would have been interrupted.
//!
//! The game never returns from its main loop, so the waits are `async`: the whole game is one
//! future, and `Runner` polls it once per frame. Each wait hands over the finished frame,
//! suspends, and picks up the next frame's buttons when polled again. No threads, so it runs the
//! same on the web.

use std::cell::RefCell;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use crate::hw::{ApuSink, Joypads, Mmc1};
use crate::{layout, Game, GameSetup, Options, Rom, Saved, Vars};

pub const WIDTH: usize = ppu::WIDTH;
pub const HEIGHT: usize = ppu::HEIGHT;

/// What the game hands over at the end of each frame.
pub struct FrameOut {
    /// The frame as NES palette indices.
    pub pixels: Box<[u8; WIDTH * HEIGHT]>,
    /// Audio for the frame (mono, [-1, 1]).
    pub samples: Vec<f32>,
    /// All variables, laid out as NES RAM (for the oracle).
    pub ram: [u8; 0x800],
    /// Video memory at the end of the frame: nametables, palette, sprites (for the oracle).
    pub vram: Box<[u8; 0x800]>,
    pub palette: [u8; 32],
    pub oam: [u8; 256],
    /// EXIT was chosen on the title menu.
    pub quit: bool,
    /// A game started with `Runner::start_game` has ended: the curtain is down, or a B-type game
    /// reached its goal. The game stays on its last screen.
    pub game_over: bool,
    /// The settings, when the settings screen changed them this frame.
    pub config: Option<crate::Config>,
    /// What the player asked for on the VERSUS screens this frame.
    #[cfg(feature = "versus")]
    pub versus: Vec<crate::versus::VersusRequest>,
}

struct FrameIn {
    buttons: u8,
    /// The app's keys, translated per screen (`pc_input`); none for the NES buttons above.
    keys: Option<crate::pc_input::Keys>,
    pokes: Vec<(u16, u8)>,
    /// What the VERSUS screens show (`Runner::next_frame_versus`).
    #[cfg(feature = "versus")]
    versus: Option<crate::versus::VersusView>,
}

/// The frame exchange between the game and `Runner`: each side leaves its half here.
#[derive(Default)]
pub(crate) struct FrameIo {
    out: RefCell<Option<FrameOut>>,
    input: RefCell<Option<FrameIn>>,
}

/// Suspends the game once: pending on the first poll, done on the next.
struct NextFrame(bool);

impl Future for NextFrame {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _: &mut Context) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            Poll::Pending
        }
    }
}

/// Run a game routine that doesn't end a frame (the waits only suspend under `Runner`).
pub fn now<T>(routine: impl Future<Output = T>) -> T {
    let mut routine = std::pin::pin!(routine);
    match routine.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("routine waited for a frame outside the runner"),
    }
}

/// The synthesiser as an APU sink: it renders each frame's audio at the frame's end.
struct Synth {
    apu: apu::Apu,
    rom: Arc<Rom>,
    odd_frame: bool,
}

impl ApuSink for Synth {
    fn write(&mut self, addr: u16, value: u8) {
        self.apu.write(addr, value);
    }

    fn end_frame(&mut self) -> Vec<f32> {
        // An NTSC frame is 29780.5 CPU cycles.
        let cycles = if self.odd_frame { 29781 } else { 29780 };
        self.odd_frame = !self.odd_frame;
        let rom = &self.rom;
        self.apu.clock(cycles, &mut |a| if a >= 0x8000 { rom.byte(a) } else { 0 });
        self.apu.take_samples()
    }
}

/// Discards APU writes (for running without sound).
struct Silent;

impl ApuSink for Silent {
    fn write(&mut self, _: u16, _: u8) {}
}

impl Game {
    /// A powered-on console with this ROM: RAM zeroed, mapper in its reset state.
    pub fn new(rom: Arc<Rom>, apu: Box<dyn ApuSink>) -> Self {
        let mut g = Game {
            ppu: ppu::Ppu::new(rom.chr.clone()),
            rom,
            vars: Vars::from_ram(&[0; 0x800]),
            mmc1: Mmc1 { control: 0x0C, ..Mmc1::default() },
            joypads: Joypads::default(),
            apu,
            io: None,
            saved: None,
            options: Options::original(),
            setup: None,
            piece_seed: 0,
            ui: Default::default(),
        };
        g.ppu.chr_banks = g.mmc1.chr_banks();
        g.ppu.mirroring = g.mmc1.control & 3;
        g
    }

    /// The start of vblank: hand over the finished frame, take the next frame's input, and run
    /// the NMI handler.
    pub(crate) async fn vblank(&mut self) {
        if let Some(io) = self.io.clone() {
            self.ppu.render_frame();
            let mut pixels = self.ppu.frame_buf.clone();
            if self.options.menus {
                self.draw_overlays(&mut pixels[..]);
            }
            let mut ram = [0; 0x800];
            self.vars.to_ram(&mut ram);
            let out = FrameOut {
                pixels,
                samples: self.apu.end_frame(),
                ram,
                vram: Box::new(self.ppu.vram),
                palette: self.ppu.palette,
                oam: self.ppu.oam,
                quit: self.ui.quit,
                game_over: self.ui.game_over,
                config: std::mem::take(&mut self.ui.config_changed).then(|| self.options.config.clone()),
                #[cfg(feature = "versus")]
                versus: std::mem::take(&mut self.ui.versus_requests),
            };
            *io.out.borrow_mut() = Some(out);
            NextFrame(false).await;
            self.take_input(&io);
        }
        self.nmi();
    }

    /// Left and Right held together, per the rules (the original passes both).
    fn left_right(&mut self, buttons: u8) -> u8 {
        use crate::options::LeftRight;
        const LR: u8 = crate::buttons::LEFT | crate::buttons::RIGHT;
        let pressed = buttons & !self.ui.raw_buttons;
        self.ui.raw_buttons = buttons;
        if pressed & LR != 0 && pressed & LR != LR {
            self.ui.last_horizontal = pressed & LR;
        }
        if buttons & LR != LR {
            return buttons;
        }
        match self.options.config.rules.left_right {
            LeftRight::Both => buttons,
            LeftRight::Neither => buttons & !LR,
            LeftRight::Last => buttons & !LR | self.ui.last_horizontal,
        }
    }

    /// Hold the buttons `Runner` left for this frame and apply its pokes.
    fn take_input(&mut self, io: &FrameIo) {
        let input = io.input.borrow_mut().take().expect("runner left no input");
        #[cfg(feature = "versus")]
        if input.versus.is_some() {
            self.ui.versus = input.versus.clone();
        }
        let buttons = match input.keys {
            Some(keys) => {
                let b = self.translate_keys(&keys);
                self.ui.keys = keys;
                b
            }
            None => {
                self.ui.keys = Default::default();
                input.buttons
            }
        };
        self.joypads.held[0] = self.left_right(buttons);
        if !input.pokes.is_empty() {
            self.vars.with_ram(|ram| {
                for &(a, v) in &input.pokes {
                    ram[a as usize & 0x7FF] = v;
                }
            });
        }
    }

    /// `nmi`: the per-frame video work, then timers, the RNG and the joypads.
    pub fn nmi(&mut self) {
        self.vars.oam_staging_length = 0;
        self.render();
        // sleepCounter counts down to 0 and stays there.
        self.vars.sleep_counter = self.vars.sleep_counter.saturating_sub(1);
        self.copy_oam_staging_to_oam();
        self.vars.frame_counter.0 = self.vars.frame_counter.0.wrapping_add(1);
        self.step_rng();
        self.vars.ppu_scroll_x = 0;
        self.vars.ppu_scroll_y = 0;
        self.ppu_scroll(0, 0);
        self.vars.vertical_blanking_interval = 1;
        self.poll_controller_buttons();
    }

    /// `render`: the video work for the current screen.
    fn render(&mut self) {
        if self.vars.render_mode == crate::pc_menus::RENDER_SETTINGS {
            return self.render_mode_settings();
        }
        self.switch_target(layout::RENDER + 5, self.vars.render_mode);
        match self.vars.render_mode {
            0 => self.render_mode_legal_and_title_screens(),
            1 => self.render_mode_menu_screens(),
            2 => self.render_mode_congratulations_screen(),
            3 => self.render_mode_play_and_demo(),
            4 => self.render_mode_ending_animation(),
            mode => panic!("render mode {mode}"),
        }
    }

    /// `updateAudioAndWaitForNmi`: one frame. Returns the A register the original leaves (1,
    /// the `verticalBlankingInterval` it waited for).
    pub async fn update_audio_and_wait_for_nmi(&mut self) -> u8 {
        self.update_audio();
        self.vars.vertical_blanking_interval = 0;
        self.vblank().await;
        self.vars.vertical_blanking_interval
    }

    /// `updateAudioWaitForNmiAndResetOamStaging`: one frame, then clear the staged sprites.
    /// The original leaves A = $FF.
    pub async fn update_audio_wait_for_nmi_and_reset_oam_staging(&mut self) -> u8 {
        self.update_audio_and_wait_for_nmi().await;
        self.memset_page(0xFF, 0x02, 0x02);
        self.ui.clear_greyed();
        0xFF
    }

    /// `updateAudioWaitForNmiAndDisablePpuRendering`. Returns the new mask (A).
    pub async fn update_audio_wait_for_nmi_and_disable_ppu_rendering(&mut self) -> u8 {
        self.update_audio_and_wait_for_nmi().await;
        self.set_ppu_mask(self.vars.current_ppu_mask & 0xE1)
    }

    /// `updateAudioWaitForNmiAndEnablePpuRendering`. Returns the new mask (A).
    pub async fn update_audio_wait_for_nmi_and_enable_ppu_rendering(&mut self) -> u8 {
        self.update_audio_and_wait_for_nmi().await;
        self.copy_current_scroll_and_ctrl_to_ppu();
        self.set_ppu_mask(self.vars.current_ppu_mask | 0x1E)
    }

    fn set_ppu_mask(&mut self, mask: u8) -> u8 {
        self.ppu_mask(mask);
        self.vars.current_ppu_mask = mask;
        mask
    }

    /// `sleep_for_a_vblanks`: wait `frames` frames (counted by the NMI handler).
    pub async fn sleep_for_a_vblanks(&mut self, frames: u8) {
        self.vars.sleep_counter = frames;
        while self.vars.sleep_counter != 0 {
            self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
        }
    }

    /// `sleep_for_14_vblanks` (which is 20 frames: $14).
    pub async fn sleep_for_14_vblanks(&mut self) {
        self.sleep_for_a_vblanks(0x14).await;
    }
}

/// Runs the game one frame at a time.
pub struct Runner {
    io: Rc<FrameIo>,
    game: Pin<Box<dyn Future<Output = Infallible>>>,
}

impl Runner {
    /// Power on with `rom`, synthesising audio at `sample_rate` Hz if given.
    pub fn new(rom: Arc<Rom>, sample_rate: Option<u32>) -> Self {
        Self::with_saved(rom, sample_rate, None)
    }

    /// As [`Runner::new`], restoring `saved` at power-on.
    pub fn with_saved(rom: Arc<Rom>, sample_rate: Option<u32>, saved: Option<Saved>) -> Self {
        Self::with_options(rom, sample_rate, saved, Options::original())
    }

    /// As [`Runner::with_saved`], with the app's additions.
    pub fn with_options(rom: Arc<Rom>, sample_rate: Option<u32>, saved: Option<Saved>, options: Options) -> Self {
        Self::build(rom, sample_rate, saved, options, None)
    }

    /// Power on straight into a game: no legal screen, title or menus. The game is the original
    /// from the moment play begins, except that it ends at game over (`FrameOut::game_over`)
    /// instead of going on to high-score entry, and Start doesn't pause.
    pub fn start_game(rom: Arc<Rom>, sample_rate: Option<u32>, setup: GameSetup) -> Self {
        assert!(setup.level <= 19 && setup.height <= 5 && setup.music <= 3 && setup.seed.rng != 0, "{setup:?}");
        let mut options = Options::original();
        options.config.rules = setup.rules;
        options.config.next_piece = setup.next_piece;
        Self::build(rom, sample_rate, None, options, Some(setup))
    }

    fn build(rom: Arc<Rom>, sample_rate: Option<u32>, saved: Option<Saved>, options: Options, setup: Option<GameSetup>) -> Self {
        let io = Rc::new(FrameIo::default());
        let apu: Box<dyn ApuSink> = match sample_rate {
            Some(rate) => Box::new(Synth { apu: apu::Apu::new(rate), rom: rom.clone(), odd_frame: false }),
            None => Box::new(Silent),
        };
        let mut game = Game::new(rom, apu);
        game.io = Some(io.clone());
        game.saved = saved;
        game.options = options;
        game.setup = setup;
        let game = Box::pin(async move {
            // The first frame's buttons are held from power-on.
            let first = game.io.clone().unwrap();
            game.take_input(&first);
            game.reset().await
        });
        Self { io, game }
    }

    /// Hold `buttons` (see `crate::buttons`) and run to the end of the next frame.
    pub fn next_frame(&mut self, buttons: u8) -> FrameOut {
        self.next_frame_with(buttons, Vec::new())
    }

    /// As [`Runner::next_frame`], writing `pokes` into RAM at the start of the frame (tests).
    pub fn next_frame_with(&mut self, buttons: u8, pokes: Vec<(u16, u8)>) -> FrameOut {
        self.step(FrameIn { buttons, keys: None, pokes, #[cfg(feature = "versus")] versus: None })
    }

    /// Run to the end of the next frame with the app's controls: keys by name, which the game
    /// turns into NES buttons per screen (`pc_input`).
    pub fn next_frame_keys(&mut self, keys: crate::pc_input::Keys) -> FrameOut {
        self.step(FrameIn { buttons: 0, keys: Some(keys), pokes: Vec::new(), #[cfg(feature = "versus")] versus: None })
    }

    /// As [`Runner::next_frame_keys`], with what the VERSUS screens show.
    #[cfg(feature = "versus")]
    pub fn next_frame_versus(&mut self, keys: crate::pc_input::Keys, view: crate::versus::VersusView) -> FrameOut {
        self.step(FrameIn { buttons: 0, keys: Some(keys), pokes: Vec::new(), versus: Some(view) })
    }

    fn step(&mut self, input: FrameIn) -> FrameOut {
        *self.io.input.borrow_mut() = Some(input);
        let Poll::Pending = self.game.as_mut().poll(&mut Context::from_waker(Waker::noop()));
        self.io.out.borrow_mut().take().expect("the game suspended without finishing a frame")
    }
}
