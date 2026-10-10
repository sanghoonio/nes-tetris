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

    fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.period;
            self.step = (self.step + 1) & 7;
        } else {
            self.timer -= 1;
        }
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

    fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.period;
            if self.length > 0 && self.linear > 0 {
                self.step = (self.step + 1) & 31;
            }
        } else {
            self.timer -= 1;
        }
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

    fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.period;
            let tap = if self.mode { 6 } else { 1 };
            let feedback = (self.shift ^ (self.shift >> tap)) & 1;
            self.shift = (self.shift >> 1) | (feedback << 14);
        } else {
            self.timer -= 1;
        }
    }

    fn clock_length(&mut self) {
        if !self.env.loop_flag && self.length > 0 {
            self.length -= 1;
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

    fn clock_timer(&mut self, fetch: &mut dyn FnMut(u16) -> u8) {
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
            return;
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
    hp_prev_in: f32,
    hp_prev_out: f32,
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
            hp_prev_in: 0.0,
            hp_prev_out: 0.0,
            samples: Vec::with_capacity(2048),
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.out_rate as u32
    }

    /// A CPU write to $4000-$4017.
    pub fn write(&mut self, addr: u16, v: u8) {
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
        self.pulse[0].env.clock();
        self.pulse[1].env.clock();
        self.noise.env.clock();
        self.triangle.clock_linear();
    }

    fn half_frame(&mut self) {
        for p in &mut self.pulse {
            p.clock_length();
            p.clock_sweep();
        }
        self.triangle.clock_length();
        self.noise.clock_length();
    }

    /// Advance `cycles` CPU cycles. `fetch` reads CPU memory for DMC samples.
    pub fn clock(&mut self, cycles: u32, fetch: &mut dyn FnMut(u16) -> u8) {
        for _ in 0..cycles {
            self.clock_one(fetch);
        }
    }

    fn clock_one(&mut self, fetch: &mut dyn FnMut(u16) -> u8) {
        self.triangle.clock_timer();
        self.odd_cycle = !self.odd_cycle;
        if self.odd_cycle {
            self.pulse[0].clock_timer();
            self.pulse[1].clock_timer();
            self.noise.clock_timer();
        }
        self.dmc.clock_timer(fetch);

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

        self.acc += self.mix();
        self.acc_n += 1;
        self.sample_clock += 1.0;
        if self.sample_clock >= self.cycles_per_sample {
            self.sample_clock -= self.cycles_per_sample;
            let x = self.acc / self.acc_n as f32;
            self.acc = 0.0;
            self.acc_n = 0;
            // ~37 Hz high-pass, as the console's output stage does, to remove DC.
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
