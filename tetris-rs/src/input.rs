//! Reading the joypads, and replaying the demo's recorded input.

use crate::shadow::{Ret, Routine};
use crate::{buttons, layout, Game};

/// Game mode 1 is the title screen, which the demo returns to when Start is pressed in it.
const GAME_MODE_TITLE: u8 = 1;
/// Game mode 5 is the demo, which plays back (or records) the joypad input.
const GAME_MODE_DEMO: u8 = 5;
/// `demo_recording` holds this while the demo is being recorded.
const DEMO_RECORDING: u8 = 0xFF;
const DEMO_BUTTONS_HIGH: u16 = layout::DEMO_BUTTONS_TABLE >> 8;
/// The demo's button table ends where the piece-type table begins.
const DEMO_TETRIMINO_TYPE_HIGH: u16 = layout::DEMO_TETRIMINO_TYPE_TABLE >> 8;

/// Working variables of `poll_controller`: the first read's presses, kept while the second
/// read is made. Written and read within the routine only.
const POLL_SCRATCH: &[&str] = &["general_counter2", "general_counter3"];
/// `pollControllerButtons` also calls `poll_controller`, and keeps its demo entry's button
/// state in `general_counter` between reading it and storing it as the held buttons.
const BUTTONS_SCRATCH: &[&str] = &["general_counter", "general_counter2", "general_counter3"];

impl Game {
    /// `pollController_actualRead`: strobe both pads, then clock out eight bits from each. Bit 0
    /// of each pad's read goes into its newly-pressed byte; bit 1 goes into `tmp1` / `tmp2`, the
    /// expansion port's bits, which `add_expansion_port_input_as_controller_input` merges in.
    pub fn poll_controller_actual_read(&mut self) {
        // The original strobes with joy1Location + 1, then with joy1Location.
        let location = self.vars.joy1_location;
        self.joypads.write_strobe(location.wrapping_add(1));
        self.joypads.write_strobe(location);
        for _ in 0..8 {
            let pad1 = self.joypads.read(0);
            let pad2 = self.joypads.read(1);
            let v = &mut self.vars;
            // Each `rol` shifts the old bits up and the new bit in; the bit shifted out is
            // unused, so the old contents are gone after eight reads.
            v.newly_pressed_buttons_player1 = v.newly_pressed_buttons_player1 << 1 | (pad1 & 1);
            v.tmp1 = v.tmp1 << 1 | (pad1 >> 1 & 1);
            v.newly_pressed_buttons_player2 = v.newly_pressed_buttons_player2 << 1 | (pad2 & 1);
            v.tmp2 = v.tmp2 << 1 | (pad2 >> 1 & 1);
        }
    }

    /// `addExpansionPortInputAsControllerInput`: OR the expansion port's buttons into the pads'.
    pub fn add_expansion_port_input_as_controller_input(&mut self) {
        let v = &mut self.vars;
        v.newly_pressed_buttons_player1 |= v.tmp1;
        v.newly_pressed_buttons_player2 |= v.tmp2;
    }

    /// `pollController`: read the pads twice and keep only the buttons both reads saw, so a
    /// bit that flickers during the read is dropped. Then work out the presses.
    pub fn poll_controller(&mut self) {
        self.poll_controller_actual_read();
        self.add_expansion_port_input_as_controller_input();
        let first1 = self.vars.newly_pressed_buttons_player1;
        let first2 = self.vars.newly_pressed_buttons_player2;

        self.poll_controller_actual_read();
        self.add_expansion_port_input_as_controller_input();
        self.vars.newly_pressed_buttons_player1 &= first1;
        self.vars.newly_pressed_buttons_player2 &= first2;

        self.diff_old_and_new_buttons();
    }

    /// `diffOldAndNewButtons` (the tail of `pollController`): a button is newly pressed if it is
    /// down now and was not held last frame. The down buttons then become the held buttons.
    fn diff_old_and_new_buttons(&mut self) {
        // Player 2 first, then player 1, as the original's countdown loop does.
        let v = &mut self.vars;
        let now2 = v.newly_pressed_buttons_player2;
        v.newly_pressed_buttons_player2 = (now2 ^ v.held_buttons_player2) & now2;
        v.held_buttons_player2 = now2;

        let now1 = v.newly_pressed_buttons_player1;
        v.newly_pressed_buttons_player1 = (now1 ^ v.held_buttons_player1) & now1;
        v.held_buttons_player1 = now1;
    }

    /// `pollControllerButtons`: the once-per-frame input. Outside the demo it reads the pad. In
    /// the demo it replays the recorded input (a button state and a repeat count per entry), and
    /// Start leaves the demo for the title screen. With `demo_recording` set it records the pad
    /// into the demo table instead.
    pub fn poll_controller_buttons(&mut self) {
        if self.vars.game_mode != GAME_MODE_DEMO {
            self.poll_controller();
            return;
        }
        if self.vars.demo_recording == DEMO_RECORDING {
            self.record_demo_buttons();
            return;
        }

        self.poll_controller();
        if self.vars.newly_pressed_buttons_player1 == buttons::START {
            self.start_game_from_demo();
            return;
        }

        if self.vars.demo_repeats != 0 {
            // Still holding the current entry: nothing newly pressed.
            self.vars.demo_repeats -= 1;
            self.vars.newly_pressed_buttons_player1 = 0;
        } else {
            // The entry is used up: read the next button state, then its repeat count.
            let state = self.demo_byte_at_pointer();
            self.demo_buttons_table_index_incr();
            let held = self.vars.demo_held_buttons;
            self.vars.newly_pressed_buttons_player1 = (held ^ state) & state;
            self.vars.demo_held_buttons = state;
            // The repeat count is the byte after the state.
            self.vars.demo_repeats = self.demo_byte_at_pointer();
            self.demo_buttons_table_index_incr();
            // Reaching the piece table means the demo's input is spent: no new held state.
            if self.demo_pointer_high() == DEMO_TETRIMINO_TYPE_HIGH {
                return;
            }
        }
        self.vars.held_buttons_player1 = self.vars.demo_held_buttons;
    }

    /// Recording (`demo_recording` = $FF): whenever the pad's held buttons change, append the
    /// previous state and how many frames it was held to the demo table. Otherwise count frames.
    fn record_demo_buttons(&mut self) {
        // The game-mode and recording checks that brought us here are still true: nothing in
        // `poll_controller` changes them.
        self.poll_controller();
        let held = self.vars.held_buttons_player1;
        if held == self.vars.demo_held_buttons {
            self.vars.demo_repeats = self.vars.demo_repeats.wrapping_add(1);
            return;
        }
        // Write the previous state, then its repeat count, into the table at the pointer.
        let state = self.vars.demo_held_buttons;
        self.mapper_write(self.vars.demo_buttons_addr.0, state);
        self.demo_buttons_table_index_incr();
        let repeats = self.vars.demo_repeats;
        self.mapper_write(self.vars.demo_buttons_addr.0, repeats);
        self.demo_buttons_table_index_incr();
        if self.demo_pointer_high() == DEMO_TETRIMINO_TYPE_HIGH {
            return;
        }
        self.vars.demo_held_buttons = held;
        self.vars.demo_repeats = 0;
    }

    /// Start pressed in the demo: rewind the demo table pointer to its start (only the high byte
    /// is set; the low byte stays), reset the frame counter's high byte, and go to the title.
    fn start_game_from_demo(&mut self) {
        let pointer = &mut self.vars.demo_buttons_addr.0;
        *pointer = DEMO_BUTTONS_HIGH << 8 | (*pointer & 0x00FF);
        self.vars.frame_counter.0 &= 0x00FF;
        self.vars.game_mode = GAME_MODE_TITLE;
    }

    /// `demoButtonsTable_indexIncr`: move the demo table pointer on by one byte.
    pub fn demo_buttons_table_index_incr(&mut self) {
        let pointer = &mut self.vars.demo_buttons_addr.0;
        *pointer = pointer.wrapping_add(1);
    }

    /// The byte the CPU reads at the demo table pointer. The table is in ROM, but the pointer
    /// can still be below $8000 (the attract demo runs with it at $0000 after a fresh start),
    /// where the original reads RAM, mirrored every $800 bytes below $2000.
    fn demo_byte_at_pointer(&self) -> u8 {
        let addr = self.vars.demo_buttons_addr.0;
        if addr >= 0x8000 {
            return self.rom.byte(addr);
        }
        if addr >= 0x2000 {
            // Registers and unmapped space; the demo never points here.
            return 0;
        }
        let mut ram = [0u8; 0x800];
        self.vars.to_ram(&mut ram);
        ram[addr as usize & 0x7FF]
    }

    fn demo_pointer_high(&self) -> u16 {
        self.vars.demo_buttons_addr.0 >> 8
    }
}

pub const SHADOW: &[Routine] = &[
    Routine {
        name: "pollController_actualRead",
        addr: layout::POLL_CONTROLLER_ACTUAL_READ,
        run: |g, _| {
            g.poll_controller_actual_read();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "addExpansionPortInputAsControllerInput",
        addr: layout::ADD_EXPANSION_PORT_INPUT_AS_CONTROLLER_INPUT,
        run: |g, _| {
            g.add_expansion_port_input_as_controller_input();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "pollController",
        addr: layout::POLL_CONTROLLER,
        run: |g, _| {
            g.poll_controller();
            Ret::Unit
        },
        scratch: POLL_SCRATCH,
    },
    Routine {
        name: "pollControllerButtons",
        addr: layout::POLL_CONTROLLER_BUTTONS,
        run: |g, _| {
            g.poll_controller_buttons();
            Ret::Unit
        },
        scratch: BUTTONS_SCRATCH,
    },
    Routine {
        name: "demoButtonsTable_indexIncr",
        addr: layout::DEMO_BUTTONS_TABLE_INDEX_INCR,
        run: |g, _| {
            g.demo_buttons_table_index_incr();
            Ret::Unit
        },
        scratch: &[],
    },
];
