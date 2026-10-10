//! Shadow oracle: every reimplemented routine against the original, on real game states.
//!
//! The recompile plays the scripted runs from `recomp::scripts`. Each call to a registered routine
//! is replayed through the readable version from the same starting state, and the results are
//! compared: every named variable (minus scratch), the PPU, mapper and joypad state, the APU
//! writes, and the value handed back to the caller.
//!
//!     cargo test -p tetris-rs --test shadow -- --nocapture
//!     ROUTINE=shift_tetrimino cargo test -p tetris-rs --test shadow -- --nocapture

use recomp::machine::Machine;
use recomp::scripts::{self, Script};
use recomp::stepper::Stepper;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tetris_rs::shadow::{Ret, Routine};
use tetris_rs::vars::{Player, RamField, SCRATCH};
use tetris_rs::{ApuSink, Game, Joypads, Mmc1, Rom, Vars};

fn rom_bytes() -> Vec<u8> {
    let path = std::env::var_os("TETRIS_ROM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tetris.nes"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("ROM {}: {e}", path.display()))
}

/// APU writes captured from the readable routine.
#[derive(Clone, Default)]
struct ApuLog(Arc<Mutex<Vec<(u16, u8)>>>);

impl ApuSink for ApuLog {
    fn write(&mut self, addr: u16, value: u8) {
        self.0.lock().unwrap().push((addr, value));
    }
}

fn game_from_machine(rom: &Arc<Rom>, m: &Machine, apu: ApuLog) -> Game {
    let (shift, strobe) = m.joypad_state();
    Game {
        rom: rom.clone(),
        vars: Vars::from_ram(&m.ram),
        ppu: m.ppu.clone(),
        mmc1: Mmc1 { control: m.mmc1_control, chr0: m.mmc1_chr0, chr1: m.mmc1_chr1, prg: m.mmc1_prg, shift: m.mmc1_shift, count: m.mmc1_count },
        joypads: Joypads { held: m.buttons, shift, strobe },
        apu: Box::new(apu),
    }
}

/// Differences between the readable result and the original's, as readable lines.
fn diff(routine: &Routine, game: &Game, ret: Ret, after: &Machine, apu: &[(u16, u8)], orig_apu: &[(u16, u8)]) -> Vec<String> {
    let mut out = Vec::new();
    let skip = |name: &str| SCRATCH.contains(&name) || routine.scratch.contains(&name);

    let mut ours = [0u8; 0x800];
    game.vars.to_ram(&mut ours);
    let theirs = &after.ram;
    for &(name, addr, size) in Vars::LAYOUT {
        if skip(name) {
            continue;
        }
        if ["active", "player1", "player2"].contains(&name) {
            for &(field, off, fsize) in Player::LAYOUT {
                let r = addr + off..addr + off + fsize;
                if ours[r.clone()] != theirs[r.clone()] && !skip(field) {
                    out.push(format!("{name}.{field}: ours {:02X?} original {:02X?}", &ours[r.clone()], &theirs[r]));
                }
            }
            continue;
        }
        let r = addr..addr + size;
        if ours[r.clone()] != theirs[r.clone()] {
            let firsts: Vec<String> = r
                .clone()
                .filter(|&a| ours[a] != theirs[a])
                .take(4)
                .map(|a| format!("[{}] ours {:02X} original {:02X}", a - addr, ours[a], theirs[a]))
                .collect();
            out.push(format!("{name}: {}", firsts.join(", ")));
        }
    }

    let (p, q) = (&game.ppu, &after.ppu);
    if p.vram != q.vram {
        let a = (0..p.vram.len()).find(|&i| p.vram[i] != q.vram[i]).unwrap();
        out.push(format!("PPU nametable byte {a:03X}: ours {:02X} original {:02X}", p.vram[a], q.vram[a]));
    }
    if p.palette != q.palette {
        out.push(format!("PPU palette: ours {:02X?} original {:02X?}", p.palette, q.palette));
    }
    if p.oam != q.oam {
        let a = (0..256).find(|&i| p.oam[i] != q.oam[i]).unwrap();
        out.push(format!("OAM[{a}]: ours {:02X} original {:02X}", p.oam[a], q.oam[a]));
    }
    for (what, ours, theirs) in [
        ("PPUCTRL", p.ctrl as u16, q.ctrl as u16),
        ("PPUMASK", p.mask as u16, q.mask as u16),
        ("PPU address v", p.v, q.v),
        ("PPU address t", p.t, q.t),
        ("PPU write toggle", p.w as u16, q.w as u16),
        ("MMC1 control", game.mmc1.control as u16, after.mmc1_control as u16),
        ("MMC1 CHR0", game.mmc1.chr0 as u16, after.mmc1_chr0 as u16),
        ("MMC1 CHR1", game.mmc1.chr1 as u16, after.mmc1_chr1 as u16),
        ("MMC1 PRG", game.mmc1.prg as u16, after.mmc1_prg as u16),
        ("MMC1 shift", game.mmc1.shift as u16, after.mmc1_shift as u16),
        ("MMC1 count", game.mmc1.count as u16, after.mmc1_count as u16),
    ] {
        if ours != theirs {
            out.push(format!("{what}: ours {ours:04X} original {theirs:04X}"));
        }
    }
    let (shift, strobe) = after.joypad_state();
    if game.joypads.shift != shift || game.joypads.strobe != strobe {
        out.push(format!("joypads: ours {:?} original shift {shift:02X?} strobe {strobe}", game.joypads));
    }
    if apu != orig_apu {
        out.push(format!("APU writes: ours {apu:02X?} original {orig_apu:02X?}"));
    }
    match ret {
        Ret::Unit => {}
        Ret::Zero(z) if z != after.z => out.push(format!("returned Z={z}, original Z={}", after.z)),
        Ret::A(a) if a != after.a => out.push(format!("returned A={a:02X}, original A={:02X}", after.a)),
        _ => {}
    }
    out
}

#[derive(Default)]
struct Tally {
    calls: usize,
    mismatches: usize,
    /// First few mismatches: (script, frame, differences, the inputs that differ from a match).
    examples: Vec<String>,
}

fn run_script(name: &str, frames: usize, mut script: Script, routines: &[&'static Routine], tallies: &Arc<Mutex<BTreeMap<&'static str, Tally>>>) {
    let rom = Arc::new(Rom::new(&rom_bytes()).expect("ROM"));
    let frame = Arc::new(Mutex::new(0usize));
    let by_addr: BTreeMap<u16, &'static Routine> = routines.iter().map(|r| (r.addr, *r)).collect();
    let (t, fr, script_name) = (tallies.clone(), frame.clone(), name.to_string());
    let mut stepper = Stepper::with_setup(&rom_bytes(), None, move |m| {
        for &a in by_addr.keys() {
            m.watch(a);
        }
        m.shadow = Some(Box::new(move |call| {
            let routine = by_addr[&call.addr];
            let log = ApuLog::default();
            let mut game = game_from_machine(&rom, &call.before, log.clone());
            let ret = (routine.run)(&mut game);
            let apu = log.0.lock().unwrap().clone();
            let diffs = diff(routine, &game, ret, call.after, &apu, &call.apu_writes);
            let mut tallies = t.lock().unwrap();
            let tally = tallies.entry(routine.name).or_default();
            tally.calls += 1;
            if !diffs.is_empty() {
                tally.mismatches += 1;
                if tally.examples.len() < 3 {
                    let b = &call.before;
                    tally.examples.push(format!(
                        "{script_name} frame {}: A={:02X} X={:02X} Y={:02X}\n      {}",
                        *fr.lock().unwrap(), b.a, b.x, b.y, diffs.join("\n      ")
                    ));
                }
            }
        }));
    });
    let mut ram = [0u8; 0x800];
    for f in 0..frames {
        *frame.lock().unwrap() = f;
        let (buttons, pokes) = script(f, &ram);
        ram = stepper.next_frame_with(buttons, false, pokes).ram;
    }
}

#[test]
fn shadow() {
    let filter = std::env::var("ROUTINE").ok();
    let routines: Vec<&'static Routine> = tetris_rs::shadow::all()
        .into_iter()
        .filter(|r| filter.as_deref().map_or(true, |f| f.split(',').any(|f| f == r.name)))
        .collect();
    assert!(!routines.is_empty(), "no registered routine matches ROUTINE={filter:?}");
    let tallies = Arc::new(Mutex::new(BTreeMap::new()));
    let runs: Vec<(&str, usize, Script)> = vec![
        ("attract", 4000, scripts::attract()),
        ("random-1", 12000, scripts::random_play(1)),
        ("random-2", 12000, scripts::random_play(0x9E3779B97F4A7C15)),
        ("level-29", 6000, scripts::level_29(29)),
        ("line-clears", 15000, scripts::line_clears(3)),
    ];
    std::thread::scope(|s| {
        for (name, frames, script) in runs {
            let (routines, tallies) = (&routines, &tallies);
            s.spawn(move || run_script(name, frames, script, routines, tallies));
        }
    });
    let tallies = tallies.lock().unwrap();
    let mut failed = false;
    for r in &routines {
        match tallies.get(r.name) {
            None => {
                println!("{:45} never called", r.name);
                failed = true;
            }
            Some(t) => {
                println!("{:45} {}/{} calls match", r.name, t.calls - t.mismatches, t.calls);
                for e in &t.examples {
                    println!("    {e}");
                }
                failed |= t.mismatches > 0;
            }
        }
    }
    assert!(!failed, "shadow mismatches (see above)");
}
