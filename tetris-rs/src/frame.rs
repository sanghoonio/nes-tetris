//! Frames: the NMI handler, the vblank waits, and running the game on its own thread.
//!
//! The original runs its game logic in a main loop and does its per-frame video work in the NMI
//! handler, which the PPU triggers at the start of vblank. The game only ever idles in two wait
//! loops when that happens (`recomp/tests/nmi_sites.rs` checks this), so here the waits call the
//! handler themselves: a frame ends exactly where the original would have been interrupted.
//!
//! The game never returns from its main loop, so `Runner` runs it on a thread and trades the
//! finished frame for the next frame's buttons at each wait.

use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;

use crate::hw::{ApuSink, Joypads, Mmc1};
use crate::{layout, Game, Rom, Vars};

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
}

struct FrameIn {
    buttons: u8,
    pokes: Vec<(u16, u8)>,
}

/// The game thread's end of the frame exchange.
pub(crate) struct FrameIo {
    out: SyncSender<FrameOut>,
    input: Receiver<FrameIn>,
}

/// Payload for unwinding the game thread when the runner goes away.
struct Stop;

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
        };
        g.ppu.chr_banks = g.mmc1.chr_banks();
        g.ppu.mirroring = g.mmc1.control & 3;
        g
    }

    /// The start of vblank: hand over the finished frame, take the next frame's input, and run
    /// the NMI handler.
    pub(crate) fn vblank(&mut self) {
        if let Some(io) = &self.io {
            self.ppu.render_frame();
            let mut ram = [0; 0x800];
            self.vars.to_ram(&mut ram);
            let out = FrameOut { pixels: self.ppu.frame_buf.clone(), samples: self.apu.end_frame(), ram };
            if io.out.send(out).is_err() {
                std::panic::resume_unwind(Box::new(Stop));
            }
            let Ok(input) = io.input.recv() else { std::panic::resume_unwind(Box::new(Stop)) };
            self.joypads.held[0] = input.buttons;
            if !input.pokes.is_empty() {
                self.vars.with_ram(|ram| {
                    for &(a, v) in &input.pokes {
                        ram[a as usize & 0x7FF] = v;
                    }
                });
            }
        }
        self.nmi();
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
    pub fn update_audio_and_wait_for_nmi(&mut self) -> u8 {
        self.update_audio();
        self.vars.vertical_blanking_interval = 0;
        self.vblank();
        self.vars.vertical_blanking_interval
    }

    /// `updateAudioWaitForNmiAndResetOamStaging`: one frame, then clear the staged sprites.
    /// The original leaves A = $FF.
    pub fn update_audio_wait_for_nmi_and_reset_oam_staging(&mut self) -> u8 {
        self.update_audio_and_wait_for_nmi();
        self.memset_page(0xFF, 0x02, 0x02);
        0xFF
    }

    /// `updateAudioWaitForNmiAndDisablePpuRendering`. Returns the new mask (A).
    pub fn update_audio_wait_for_nmi_and_disable_ppu_rendering(&mut self) -> u8 {
        self.update_audio_and_wait_for_nmi();
        self.set_ppu_mask(self.vars.current_ppu_mask & 0xE1)
    }

    /// `updateAudioWaitForNmiAndEnablePpuRendering`. Returns the new mask (A).
    pub fn update_audio_wait_for_nmi_and_enable_ppu_rendering(&mut self) -> u8 {
        self.update_audio_and_wait_for_nmi();
        self.copy_current_scroll_and_ctrl_to_ppu();
        self.set_ppu_mask(self.vars.current_ppu_mask | 0x1E)
    }

    fn set_ppu_mask(&mut self, mask: u8) -> u8 {
        self.ppu_mask(mask);
        self.vars.current_ppu_mask = mask;
        mask
    }

    /// `sleep_for_a_vblanks`: wait `frames` frames (counted by the NMI handler).
    pub fn sleep_for_a_vblanks(&mut self, frames: u8) {
        self.vars.sleep_counter = frames;
        while self.vars.sleep_counter != 0 {
            self.update_audio_wait_for_nmi_and_reset_oam_staging();
        }
    }

    /// `sleep_for_14_vblanks` (which is 20 frames: $14).
    pub fn sleep_for_14_vblanks(&mut self) {
        self.sleep_for_a_vblanks(0x14);
    }
}

/// Runs the game on its own thread, one frame at a time.
pub struct Runner {
    input: Option<SyncSender<FrameIn>>,
    output: Receiver<FrameOut>,
    thread: Option<JoinHandle<()>>,
}

impl Runner {
    /// Power on with `rom`, synthesising audio at `sample_rate` Hz if given.
    pub fn new(rom: Arc<Rom>, sample_rate: Option<u32>) -> Self {
        let (in_tx, in_rx) = sync_channel::<FrameIn>(0);
        let (out_tx, out_rx) = sync_channel::<FrameOut>(0);
        let thread = std::thread::Builder::new()
            .name("tetris-rs".into())
            .stack_size(16 << 20)
            .spawn(move || {
                let apu: Box<dyn ApuSink> = match sample_rate {
                    Some(rate) => Box::new(Synth { apu: apu::Apu::new(rate), rom: rom.clone(), odd_frame: false }),
                    None => Box::new(Silent),
                };
                let mut game = Game::new(rom, apu);
                let Ok(first) = in_rx.recv() else { return };
                game.joypads.held[0] = first.buttons;
                game.io = Some(FrameIo { out: out_tx, input: in_rx });
                game.reset();
            })
            .expect("spawn game thread");
        Self { input: Some(in_tx), output: out_rx, thread: Some(thread) }
    }

    /// Hold `buttons` (see `crate::buttons`) and run to the end of the next frame.
    pub fn next_frame(&mut self, buttons: u8) -> FrameOut {
        self.next_frame_with(buttons, Vec::new())
    }

    /// As [`Runner::next_frame`], writing `pokes` into RAM at the start of the frame (tests).
    pub fn next_frame_with(&mut self, buttons: u8, pokes: Vec<(u16, u8)>) -> FrameOut {
        let sent = self.input.as_ref().expect("runner stopped").send(FrameIn { buttons, pokes });
        match sent.ok().and_then(|_| self.output.recv().ok()) {
            Some(out) => out,
            None => {
                // The game thread died: surface its panic here.
                if let Some(t) = self.thread.take() {
                    if let Err(e) = t.join() {
                        std::panic::resume_unwind(e);
                    }
                }
                panic!("game thread exited");
            }
        }
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        // Closing the channels unwinds the game thread at its next frame; it isn't joined, so a
        // blocked send can't hang the drop.
        self.input.take();
        self.thread.take();
    }
}
