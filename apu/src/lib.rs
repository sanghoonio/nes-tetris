//! NES 2A03 APU (NTSC): two pulse channels, triangle, noise and DMC, the frame sequencer, and the
//! standard non-linear mixer, resampled to a fixed output rate.
//!
//! Driven by register writes (`write`) and CPU cycles (`clock`). The DMC fetches sample bytes
//! through a callback so the APU needs no copy of the cartridge.

pub const CPU_HZ: f64 = 1_789_773.0;

const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];
const DUTY: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0],
    [0, 1, 1, 0, 0, 0, 0, 0],
    [0, 1, 1, 1, 1, 0, 0, 0],
    [1, 0, 0, 1, 1, 1, 1, 1],
];
const TRIANGLE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
    13, 14, 15,
];
const NOISE_PERIOD: [u16; 16] = [4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068];
const DMC_RATE: [u16; 16] = [428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54];

/// Clock a channel timer (reloaded with `period` when it reaches 0) `k` times at once. Returns
/// how many times it reloaded.
fn advance_timer(timer: &mut u16, period: u16, k: u32) -> u32 {
    let t = *timer as u32;
    if k <= t {
        *timer -= k as u16;
        return 0;
    }
    let after_first = k - t - 1;
    let len = period as u32 + 1;
    *timer = (period as u32 - after_first % len) as u16;
    1 + after_first / len
}

#[derive(Default, Clone)]
struct Envelope {
    start: bool,
    loop_flag: bool,
    constant: bool,
    volume: u8,
    divider: u8,
    decay: u8,
}

impl Envelope {
    fn write(&mut self, v: u8) {
        self.loop_flag = v & 0x20 != 0;
        self.constant = v & 0x10 != 0;
        self.volume = v & 0x0F;
    }

    fn clock(&mut self) {
        if self.start {
            self.start = false;
            self.decay = 15;
            self.divider = self.volume;
        } else if self.divider == 0 {
            self.divider = self.volume;
            if self.decay > 0 {
                self.decay -= 1;
            } else if self.loop_flag {
                self.decay = 15;
            }
        } else {
            self.divider -= 1;
        }
    }

    fn output(&self) -> u8 {
        if self.constant { self.volume } else { self.decay }
    }
}

#[derive(Default, Clone)]
struct Pulse {
    second: bool,
    enabled: bool,
    duty: u8,
    step: u8,
    period: u16,
    timer: u16,
    length: u8,
    env: Envelope,
    sweep_enabled: bool,
    sweep_period: u8,
    sweep_negate: bool,
    sweep_shift: u8,
    sweep_divider: u8,
    sweep_reload: bool,
}

impl Pulse {
    fn write(&mut self, reg: u16, v: u8) {
        match reg {
            0 => {
                self.duty = v >> 6;
                self.env.write(v);
            }
            1 => {
                self.sweep_enabled = v & 0x80 != 0;
                self.sweep_period = (v >> 4) & 7;
                self.sweep_negate = v & 0x08 != 0;
                self.sweep_shift = v & 7;
                self.sweep_reload = true;
            }
            2 => self.period = (self.period & 0x700) | v as u16,
            _ => {
                self.period = (self.period & 0xFF) | ((v as u16 & 7) << 8);
                if self.enabled {
                    self.length = LENGTH_TABLE[(v >> 3) as usize];
                }
                self.step = 0;
                self.env.start = true;
            }
        }
    }

    fn target_period(&self) -> u16 {
        let change = self.period >> self.sweep_shift;
        if self.sweep_negate {
            // Pulse 1 negates with one's complement, pulse 2 with two's.
            self.period.wrapping_sub(change).wrapping_sub(!self.second as u16)
        } else {
            self.period + change
        }
    }

    fn muted(&self) -> bool {
        self.period < 8 || self.target_period() > 0x7FF
    }

    /// Returns whether the output may have changed.
    fn clock_timer(&mut self) -> bool {
        if self.timer == 0 {
            self.timer = self.period;
            self.step = (self.step + 1) & 7;
            true
        } else {
            self.timer -= 1;
            false
        }
    }

    /// `k` timer clocks at once.
    fn skip(&mut self, k: u32) {
        let reloads = advance_timer(&mut self.timer, self.period, k);
        self.step = ((self.step as u32 + reloads) & 7) as u8;
    }

    /// Whether a step of the waveform can change the output.
    fn audible(&self) -> bool {
        self.length > 0 && !self.muted()
    }

    fn clock_sweep(&mut self) {
        if self.sweep_divider == 0 && self.sweep_enabled && self.sweep_shift > 0 && !self.muted() {
            self.period = self.target_period();
        }
        if self.sweep_divider == 0 || self.sweep_reload {
            self.sweep_divider = self.sweep_period;
            self.sweep_reload = false;
        } else {
            self.sweep_divider -= 1;
        }
    }

    fn clock_length(&mut self) {
        if !self.env.loop_flag && self.length > 0 {
            self.length -= 1;
        }
    }

    fn output(&self) -> u8 {
        if self.length == 0 || self.muted() || DUTY[self.duty as usize][self.step as usize] == 0 {
            0
        } else {
            self.env.output()
        }
    }
}

#[derive(Default, Clone)]
struct Triangle {
    enabled: bool,
    control: bool,
    linear_reload_value: u8,
    linear: u8,
    linear_reload: bool,
    period: u16,
    timer: u16,
    step: u8,
    length: u8,
}

impl Triangle {
    fn write(&mut self, reg: u16, v: u8) {
        match reg {
            0 => {
                self.control = v & 0x80 != 0;
                self.linear_reload_value = v & 0x7F;
            }
            2 => self.period = (self.period & 0x700) | v as u16,
            3 => {
                self.period = (self.period & 0xFF) | ((v as u16 & 7) << 8);
                if self.enabled {
                    self.length = LENGTH_TABLE[(v >> 3) as usize];
                }
                self.linear_reload = true;
            }
            _ => {}
        }
    }

    /// Returns whether the output may have changed.
    fn clock_timer(&mut self) -> bool {
        if self.timer == 0 {
            self.timer = self.period;
            if self.length > 0 && self.linear > 0 {
                self.step = (self.step + 1) & 31;
            }
            true
        } else {
            self.timer -= 1;
            false
        }
    }

    /// `k` timer clocks at once.
    fn skip(&mut self, k: u32) {
        let reloads = advance_timer(&mut self.timer, self.period, k);
        if self.length > 0 && self.linear > 0 {
            self.step = ((self.step as u32 + reloads) & 31) as u8;
        }
    }

    /// Whether a timer reload can change the output.
    fn audible(&self) -> bool {
        self.length > 0 && self.linear > 0 && self.period >= 2
    }

    fn clock_linear(&mut self) {
        if self.linear_reload {
            self.linear = self.linear_reload_value;
        } else if self.linear > 0 {
            self.linear -= 1;
        }
        if !self.control {
            self.linear_reload = false;
        }
    }

    fn clock_length(&mut self) {
        if !self.control && self.length > 0 {
            self.length -= 1;
        }
    }

    fn output(&self) -> u8 {
        // Ultrasonic periods are silenced rather than emulated as a DC-ish buzz.
        if self.period < 2 { 7 } else { TRIANGLE[self.step as usize] }
    }
}

#[derive(Clone)]
struct Noise {
    enabled: bool,
    mode: bool,
    period: u16,
    timer: u16,
    shift: u16,
    length: u8,
    env: Envelope,
}

impl Noise {
    fn write(&mut self, reg: u16, v: u8) {
        match reg {
            0 => self.env.write(v),
            2 => {
                self.mode = v & 0x80 != 0;
                self.period = NOISE_PERIOD[(v & 0x0F) as usize];
            }
            3 => {
                if self.enabled {
                    self.length = LENGTH_TABLE[(v >> 3) as usize];
                }
                self.env.start = true;
            }
            _ => {}
        }
    }

    /// Returns whether the output may have changed.
    fn clock_timer(&mut self) -> bool {
        if self.timer == 0 {
            self.timer = self.period;
            let tap = if self.mode { 6 } else { 1 };
            let feedback = (self.shift ^ (self.shift >> tap)) & 1;
            self.shift = (self.shift >> 1) | (feedback << 14);
            true
        } else {
            self.timer -= 1;
            false
        }
    }

    fn clock_length(&mut self) {
        if !self.env.loop_flag && self.length > 0 {
            self.length -= 1;
        }
    }

    /// `k` timer clocks at once.
    fn skip(&mut self, k: u32) {
        let tap = if self.mode { 6 } else { 1 };
        for _ in 0..advance_timer(&mut self.timer, self.period, k) {
            let feedback = (self.shift ^ (self.shift >> tap)) & 1;
            self.shift = (self.shift >> 1) | (feedback << 14);
        }
    }

    fn output(&self) -> u8 {
        if self.length == 0 || self.shift & 1 != 0 { 0 } else { self.env.output() }
    }
}

#[derive(Default, Clone)]
struct Dmc {
    loop_flag: bool,
    period: u16,
    timer: u16,
    level: u8,
    sample_addr: u16,
    sample_len: u16,
    addr: u16,
    remaining: u16,
    buffer: Option<u8>,
    shift: u8,
    bits: u8,
    silent: bool,
}

impl Dmc {
    fn write(&mut self, reg: u16, v: u8) {
        match reg {
            0 => {
                self.loop_flag = v & 0x40 != 0;
                self.period = DMC_RATE[(v & 0x0F) as usize];
            }
            1 => self.level = v & 0x7F,
            2 => self.sample_addr = 0xC000 | (v as u16) << 6,
            _ => self.sample_len = (v as u16) << 4 | 1,
        }
    }

    fn restart(&mut self) {
        self.addr = self.sample_addr;
        self.remaining = self.sample_len;
    }

    /// Returns whether the output may have changed.
    fn clock_timer(&mut self, fetch: &mut dyn FnMut(u16) -> u8) -> bool {
        if self.buffer.is_none() && self.remaining > 0 {
            self.buffer = Some(fetch(self.addr));
            self.addr = if self.addr == 0xFFFF { 0x8000 } else { self.addr + 1 };
            self.remaining -= 1;
            if self.remaining == 0 && self.loop_flag {
                self.restart();
            }
        }
        if self.timer > 0 {
            self.timer -= 1;
            return false;
        }
        self.timer = self.period.saturating_sub(1);
        if !self.silent {
            if self.shift & 1 != 0 {
                if self.level <= 125 {
                    self.level += 2;
                }
            } else if self.level >= 2 {
                self.level -= 2;
            }
        }
        self.shift >>= 1;
        if self.bits == 0 {
            self.bits = 8;
            match self.buffer.take() {
                Some(b) => {
                    self.silent = false;
                    self.shift = b;
                }
                None => self.silent = true,
            }
        }
        self.bits -= 1;
        true
    }
}

#[derive(Clone)]
pub struct Apu {
    pulse: [Pulse; 2],
    triangle: Triangle,
    noise: Noise,
    dmc: Dmc,
    five_step: bool,
    frame_cycle: u32,
    odd_cycle: bool,

    out_rate: f64,
    cycles_per_sample: f64,
    sample_clock: f64,
    acc: f32,
    acc_n: u32,
    /// The mixer's output, recomputed only when a channel's output may have changed: every
    /// cycle's value is the same as mixing afresh, at a fraction of the cost.
    mixed: f32,
    stale: bool,
    hp_prev_in: f32,
    hp_prev_out: f32,
    /// Whether the high-pass has had its first input. The mix has a DC offset from power-on (the
    /// idle triangle sits mid-level); starting the filter at that level instead of at 0 keeps it
    /// from coming out as a pop.
    hp_primed: bool,
    /// Mono samples in [-1, 1] produced since the last [`Apu::take_samples`].
    samples: Vec<f32>,
}

impl Apu {
    pub fn new(out_rate: u32) -> Self {
        Self {
            pulse: [Pulse::default(), Pulse { second: true, ..Pulse::default() }],
            triangle: Triangle::default(),
            noise: Noise {
                enabled: false,
                mode: false,
                period: NOISE_PERIOD[0],
                timer: 0,
                shift: 1,
                length: 0,
                env: Envelope::default(),
            },
            dmc: Dmc { period: DMC_RATE[0], bits: 8, silent: true, ..Dmc::default() },
            five_step: false,
            frame_cycle: 0,
            odd_cycle: false,
            out_rate: out_rate as f64,
            cycles_per_sample: CPU_HZ / out_rate as f64,
            sample_clock: 0.0,
            acc: 0.0,
            acc_n: 0,
            mixed: 0.0,
            stale: true,
            hp_prev_in: 0.0,
            hp_prev_out: 0.0,
            hp_primed: false,
            samples: Vec::with_capacity(2048),
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.out_rate as u32
    }

    /// A CPU write to $4000-$4017.
    pub fn write(&mut self, addr: u16, v: u8) {
        self.stale = true;
        match addr {
            0x4000..=0x4003 => self.pulse[0].write(addr - 0x4000, v),
            0x4004..=0x4007 => self.pulse[1].write(addr - 0x4004, v),
            0x4008..=0x400B => self.triangle.write(addr - 0x4008, v),
            0x400C..=0x400F => self.noise.write(addr - 0x400C, v),
            0x4010..=0x4013 => self.dmc.write(addr - 0x4010, v),
            0x4015 => {
                self.pulse[0].enabled = v & 1 != 0;
                self.pulse[1].enabled = v & 2 != 0;
                self.triangle.enabled = v & 4 != 0;
                self.noise.enabled = v & 8 != 0;
                for (i, p) in self.pulse.iter_mut().enumerate() {
                    if v & (1 << i) == 0 {
                        p.length = 0;
                    }
                }
                if v & 4 == 0 {
                    self.triangle.length = 0;
                }
                if v & 8 == 0 {
                    self.noise.length = 0;
                }
                if v & 0x10 == 0 {
                    self.dmc.remaining = 0;
                } else if self.dmc.remaining == 0 {
                    self.dmc.restart();
                }
            }
            0x4017 => {
                self.five_step = v & 0x80 != 0;
                self.frame_cycle = 0;
                if self.five_step {
                    self.quarter_frame();
                    self.half_frame();
                }
            }
            _ => {}
        }
    }

    fn quarter_frame(&mut self) {
        self.stale = true;
        self.pulse[0].env.clock();
        self.pulse[1].env.clock();
        self.noise.env.clock();
        self.triangle.clock_linear();
    }

    fn half_frame(&mut self) {
        self.stale = true;
        for p in &mut self.pulse {
            p.clock_length();
            p.clock_sweep();
        }
        self.triangle.clock_length();
        self.noise.clock_length();
    }

    /// Advance `cycles` CPU cycles. `fetch` reads CPU memory for DMC samples.
    pub fn clock(&mut self, cycles: u32, fetch: &mut dyn FnMut(u16) -> u8) {
        let mut left = cycles;
        while left > 0 {
            let quiet = self.quiet_cycles().min(left - 1);
            if quiet > 0 {
                self.skip(quiet);
                left -= quiet;
            }
            self.clock_one::<false>(fetch);
            left -= 1;
        }
    }

    /// How many of the coming cycles can't change the output or need anything but their timers
    /// counted: none of the audible channels steps its waveform, the DMC neither fetches nor
    /// shifts, the frame sequencer doesn't fire, and no output sample is due.
    fn quiet_cycles(&self) -> u32 {
        let d = &self.dmc;
        if d.buffer.is_none() && d.remaining > 0 {
            return 0;
        }
        // The cycle (1 = the next) of each coming event.
        let mut next = d.timer as u32 + 1;
        if self.triangle.audible() {
            next = next.min(self.triangle.timer as u32 + 1);
        }
        // Pulses and noise are clocked on odd cycles only.
        let first_odd = if self.odd_cycle { 2 } else { 1 };
        for p in &self.pulse {
            if p.audible() {
                next = next.min(first_odd + 2 * p.timer as u32);
            }
        }
        if self.noise.length > 0 {
            next = next.min(first_odd + 2 * self.noise.timer as u32);
        }
        let end = if self.five_step { 37281 } else { 29829 };
        let step = [7457, 14913, 22371, end].into_iter().find(|&c| c > self.frame_cycle).unwrap_or(end);
        next = next.min(step - self.frame_cycle);
        next = next.min((self.cycles_per_sample - self.sample_clock).ceil().max(1.0) as u32);
        next - 1
    }

    /// `k` quiet cycles (see `quiet_cycles`) at once.
    fn skip(&mut self, k: u32) {
        if self.stale {
            self.mixed = self.mix();
            self.stale = false;
        }
        self.triangle.skip(k);
        let odd = if self.odd_cycle { k / 2 } else { k.div_ceil(2) };
        if k % 2 == 1 {
            self.odd_cycle = !self.odd_cycle;
        }
        self.pulse[0].skip(odd);
        self.pulse[1].skip(odd);
        self.noise.skip(odd);
        self.dmc.timer -= k as u16;
        self.frame_cycle += k;
        self.acc += self.mixed * k as f32;
        self.acc_n += k;
        self.sample_clock += k as f64;
    }

    /// As `clock`, mixing afresh every cycle: the reference the cached mix is tested against.
    #[cfg(test)]
    fn clock_reference(&mut self, cycles: u32, fetch: &mut dyn FnMut(u16) -> u8) {
        for _ in 0..cycles {
            self.clock_one::<true>(fetch);
        }
    }

    fn clock_one<const ALWAYS_MIX: bool>(&mut self, fetch: &mut dyn FnMut(u16) -> u8) {
        let mut changed = self.triangle.clock_timer();
        self.odd_cycle = !self.odd_cycle;
        if self.odd_cycle {
            changed |= self.pulse[0].clock_timer();
            changed |= self.pulse[1].clock_timer();
            changed |= self.noise.clock_timer();
        }
        changed |= self.dmc.clock_timer(fetch);

        // Frame sequencer, in CPU cycles.
        self.frame_cycle += 1;
        match (self.five_step, self.frame_cycle) {
            (_, 7457) | (_, 22371) => self.quarter_frame(),
            (_, 14913) => {
                self.quarter_frame();
                self.half_frame();
            }
            (false, 29829) => {
                self.quarter_frame();
                self.half_frame();
                self.frame_cycle = 0;
            }
            (true, 37281) => {
                self.quarter_frame();
                self.half_frame();
                self.frame_cycle = 0;
            }
            _ => {}
        }

        if ALWAYS_MIX || changed || self.stale {
            self.mixed = self.mix();
            self.stale = false;
        }
        self.acc += self.mixed;
        self.acc_n += 1;
        self.sample_clock += 1.0;
        if self.sample_clock >= self.cycles_per_sample {
            self.sample_clock -= self.cycles_per_sample;
            let x = self.acc / self.acc_n as f32;
            self.acc = 0.0;
            self.acc_n = 0;
            // ~37 Hz high-pass, as the console's output stage does, to remove DC.
            if !self.hp_primed {
                self.hp_primed = true;
                self.hp_prev_in = x;
            }
            let y = 0.996 * (self.hp_prev_out + x - self.hp_prev_in);
            self.hp_prev_in = x;
            self.hp_prev_out = y;
            self.samples.push((y * 1.8).clamp(-1.0, 1.0));
        }
    }

    fn mix(&self) -> f32 {
        let p = (self.pulse[0].output() + self.pulse[1].output()) as f32;
        let pulse = if p == 0.0 { 0.0 } else { 95.88 / (8128.0 / p + 100.0) };
        let t = self.triangle.output() as f32 / 8227.0;
        let n = self.noise.output() as f32 / 12241.0;
        let d = self.dmc.level as f32 / 22638.0;
        let tnd_sum = t + n + d;
        let tnd = if tnd_sum == 0.0 { 0.0 } else { 159.79 / (1.0 / tnd_sum + 100.0) };
        pulse + tnd
    }

    /// Drain the samples generated so far.
    pub fn take_samples(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.samples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Xorshift(u64);
    impl Xorshift {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    /// Powered on and left alone, the APU is silent: its DC offset doesn't come out as a pop.
    #[test]
    fn no_pop_at_power_on() {
        let mut apu = Apu::new(48000);
        apu.clock(29780 * 10, &mut |_| 0);
        let peak = apu.take_samples().iter().map(|x| x.abs()).fold(0.0, f32::max);
        assert!(peak < 1e-6, "peak {peak}");
    }

    /// Jumping ahead through quiet cycles gives the samples clocking and mixing every cycle does
    /// (to float rounding, from adding a stretch's mix at once), under random register writes to
    /// every channel (DMC samples included) in both frame sequencer modes.
    #[test]
    fn matches_clocking_every_cycle() {
        let mut rng = Xorshift(0x2545_F491_4F6C_DD1D);
        let memory: Vec<u8> = (0..0x10000).map(|_| rng.next() as u8).collect();
        let mut fetch = |a: u16| memory[a as usize];
        let mut fetch2 = |a: u16| memory[a as usize];
        let mut apu = Apu::new(48000);
        let mut reference = Apu::new(48000);
        for frame in 0..3000 {
            for _ in 0..rng.next() % 12 {
                let addr = match rng.next() % 20 {
                    0..=17 => 0x4000 + (rng.next() % 0x14) as u16,
                    18 => 0x4015,
                    _ => 0x4017,
                };
                let v = rng.next() as u8;
                apu.write(addr, v);
                reference.write(addr, v);
            }
            let cycles = 29780 + frame % 2;
            apu.clock(cycles, &mut fetch);
            reference.clock_reference(cycles, &mut fetch2);
            let (a, b) = (apu.take_samples(), reference.take_samples());
            assert_eq!(a.len(), b.len(), "frame {frame}");
            let worst = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max);
            assert!(worst < 1e-5, "frame {frame}: off by {worst}");
        }
    }
}
