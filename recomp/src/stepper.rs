//! Run the recompiled game one frame at a time.
//!
//! The game never returns from its main loop, so it runs on its own thread and hands control back
//! at every NMI entry (the start of vblank), where it receives the next frame's buttons.

use crate::machine::Machine;
use ppu::{HEIGHT, WIDTH};

use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::thread::JoinHandle;

/// State at an NMI entry.
pub struct Snapshot {
    pub ram: [u8; 0x800],
    pub sp: u8,
    pub cycles: u64,
    /// The finished frame as NES palette indices, if rendering was requested.
    pub pixels: Option<Box<[u8; WIDTH * HEIGHT]>>,
    /// APU register writes latched so far ($4000-$4017).
    pub apu_regs: [u8; 0x18],
    /// Addresses the fallback interpreter ran since power-on.
    pub fallback_log: Vec<u16>,
    /// Audio generated since the previous snapshot (mono, [-1, 1]), if audio is enabled.
    pub samples: Vec<f32>,
}

struct Request {
    buttons: u8,
    render: bool,
    pokes: Vec<(u16, u8)>,
}

pub struct Stepper {
    tx: Option<SyncSender<Request>>,
    rx: Receiver<Snapshot>,
    thread: Option<JoinHandle<()>>,
}

/// Payload used to stop the game thread quietly when the stepper is dropped.
struct Stop;

impl Stepper {
    pub fn new(rom: &[u8]) -> Self {
        Self::with_audio(rom, None)
    }

    /// As [`Stepper::new`], synthesising audio at `sample_rate` Hz if given.
    pub fn with_audio(rom: &[u8], sample_rate: Option<u32>) -> Self {
        Self::with_setup(rom, sample_rate, |_| {})
    }

    /// As [`Stepper::with_audio`], running `setup` on the machine before power-on (to install
    /// shadow-mode watches, for instance).
    pub fn with_setup(rom: &[u8], sample_rate: Option<u32>, setup: impl FnOnce(&mut Machine) + Send + 'static) -> Self {
        let rom = rom.to_vec();
        let (req_tx, req_rx) = sync_channel::<Request>(0);
        let (snap_tx, snap_rx) = sync_channel::<Snapshot>(0);
        let thread = std::thread::Builder::new()
            .name("tetris".into())
            .stack_size(64 << 20)
            .spawn(move || {
                let mut m = Machine::new(&rom);
                m.apu = sample_rate.map(apu::Apu::new);
                setup(&mut m);
                let Ok(first) = req_rx.recv() else { return };
                m.buttons[0] = first.buttons;
                for &(a, v) in &first.pokes {
                    m.ram[a as usize & 0x7FF] = v;
                }
                let mut render = first.render;
                m.on_nmi = Some(Box::new(move |m: &mut Machine| {
                    let pixels = render.then(|| m.ppu.frame_buf.clone());
                    let snap = Snapshot {
                        ram: m.ram,
                        sp: m.sp,
                        cycles: m.cycles,
                        pixels,
                        apu_regs: m.apu_regs,
                        fallback_log: m.fallback_log.clone(),
                        samples: m.apu.as_mut().map(|a| a.take_samples()).unwrap_or_default(),
                    };
                    if snap_tx.send(snap).is_err() {
                        std::panic::resume_unwind(Box::new(Stop));
                    }
                    match req_rx.recv() {
                        Ok(req) => {
                            m.buttons[0] = req.buttons;
                            for &(a, v) in &req.pokes {
                                m.ram[a as usize & 0x7FF] = v;
                            }
                            render = req.render;
                        }
                        Err(_) => std::panic::resume_unwind(Box::new(Stop)),
                    }
                }));
                m.run();
            })
            .expect("spawn game thread");
        Self { tx: Some(req_tx), rx: snap_rx, thread: Some(thread) }
    }

    /// Hold `buttons` (see `machine::BTN_*`) and run to the next NMI entry.
    pub fn next_frame(&mut self, buttons: u8, render: bool) -> Snapshot {
        self.next_frame_with(buttons, render, Vec::new())
    }

    /// As [`Stepper::next_frame`], writing `pokes` into RAM first (for tests that skip ahead).
    pub fn next_frame_with(&mut self, buttons: u8, render: bool, pokes: Vec<(u16, u8)>) -> Snapshot {
        let tx = self.tx.as_ref().expect("stepper stopped");
        if tx.send(Request { buttons, render, pokes }).is_err() {
            self.join_and_report();
        }
        match self.rx.recv() {
            Ok(s) => s,
            Err(_) => {
                self.join_and_report();
                unreachable!()
            }
        }
    }

    /// The game thread died: re-raise its panic here.
    fn join_and_report(&mut self) {
        if let Some(t) = self.thread.take() {
            if let Err(e) = t.join() {
                std::panic::resume_unwind(e);
            }
        }
        panic!("game thread exited");
    }
}

impl Drop for Stepper {
    fn drop(&mut self) {
        // Closing the channels makes the game thread unwind with `Stop` at its next NMI; it is
        // left detached rather than joined so a blocked send can't deadlock the drop.
        self.tx.take();
        self.thread.take();
    }
}
