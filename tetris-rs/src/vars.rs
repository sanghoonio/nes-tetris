//! The game's variables, named and typed, at the RAM addresses the original keeps them.
//!
//! The reimplementation works on these fields directly. Keeping the original layout means any
//! state can be loaded from, or stored to, a 2 KB NES RAM image, which is what lets the oracle
//! compare it field by field against the recompile. Unnamed bytes in the original (padding and
//! unused RAM) aren't represented.

/// A value stored in RAM.
pub trait RamField: Sized {
    const SIZE: usize;
    fn load(bytes: &[u8]) -> Self;
    fn store(&self, bytes: &mut [u8]);
}

impl RamField for u8 {
    const SIZE: usize = 1;
    fn load(bytes: &[u8]) -> Self {
        bytes[0]
    }
    fn store(&self, bytes: &mut [u8]) {
        bytes[0] = *self;
    }
}

impl<const N: usize> RamField for [u8; N] {
    const SIZE: usize = N;
    fn load(bytes: &[u8]) -> Self {
        bytes[..N].try_into().unwrap()
    }
    fn store(&self, bytes: &mut [u8]) {
        bytes[..N].copy_from_slice(self);
    }
}

/// Little-endian 16-bit value (low byte first, as 6502 pointers and counters are).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Le16(pub u16);

impl RamField for Le16 {
    const SIZE: usize = 2;
    fn load(bytes: &[u8]) -> Self {
        Le16(u16::from_le_bytes([bytes[0], bytes[1]]))
    }
    fn store(&self, bytes: &mut [u8]) {
        bytes[..2].copy_from_slice(&self.0.to_le_bytes());
    }
}

/// Big-endian 16-bit value (the RNG seed is shifted as high byte, then low byte).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Be16(pub u16);

impl RamField for Be16 {
    const SIZE: usize = 2;
    fn load(bytes: &[u8]) -> Self {
        Be16(u16::from_be_bytes([bytes[0], bytes[1]]))
    }
    fn store(&self, bytes: &mut [u8]) {
        bytes[..2].copy_from_slice(&self.0.to_be_bytes());
    }
}

/// Packed BCD, two decimal digits per byte, least significant byte first: scores and line counts.
/// The game does its own digit arithmetic on these, including the overflow quirks, so they stay
/// as bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bcd<const N: usize>(pub [u8; N]);

impl<const N: usize> RamField for Bcd<N> {
    const SIZE: usize = N;
    fn load(bytes: &[u8]) -> Self {
        Bcd(bytes[..N].try_into().unwrap())
    }
    fn store(&self, bytes: &mut [u8]) {
        bytes[..N].copy_from_slice(&self.0);
    }
}

impl<const N: usize> Bcd<N> {
    /// The value as a plain number, reading each nibble as a digit (a nibble over 9 counts as
    /// its face value, as the game's display would show garbage for it).
    pub fn value(&self) -> u32 {
        self.0.iter().rev().fold(0, |acc, &b| acc * 100 + (b >> 4) as u32 * 10 + (b & 15) as u32)
    }
}

macro_rules! ram_struct {
    (
        $(#[$sm:meta])*
        pub struct $name:ident {
            $( $(#[$m:meta])* $field:ident : $ty:ty = $offset:literal, )*
        }
    ) => {
        $(#[$sm])*
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct $name {
            $( $(#[$m])* pub $field: $ty, )*
        }

        impl RamField for $name {
            const SIZE: usize = {
                let mut end = 0;
                $( if $offset + <$ty as RamField>::SIZE > end { end = $offset + <$ty as RamField>::SIZE; } )*
                end
            };
            fn load(bytes: &[u8]) -> Self {
                Self { $( $field: <$ty as RamField>::load(&bytes[$offset..]), )* }
            }
            fn store(&self, bytes: &mut [u8]) {
                $( self.$field.store(&mut bytes[$offset..]); )*
            }
        }

        impl $name {
            /// (field, offset, size) for every field, for oracle reports.
            pub const LAYOUT: &'static [(&'static str, usize, usize)] =
                &[ $( (stringify!($field), $offset, <$ty as RamField>::SIZE), )* ];
        }
    };
}

ram_struct! {
    /// One player's piece and progress. The game keeps three copies: `active` (what the play
    /// code works on), and `player1` / `player2`, copied to and from `active` around each
    /// player's turn.
    pub struct Player {
        tetrimino_x: u8 = 0x00,
        tetrimino_y: u8 = 0x01,
        /// Orientation id of the falling piece (0x00..=0x12), or 0x13 for "none".
        current_piece: u8 = 0x02,
        unused_03: u8 = 0x03,
        level_number: u8 = 0x04,
        fall_timer: u8 = 0x05,
        /// DAS counter for left/right.
        autorepeat_x: u8 = 0x06,
        start_level: u8 = 0x07,
        play_state: u8 = 0x08,
        vram_row: u8 = 0x09,
        completed_row: [u8; 4] = 0x0A,
        /// Soft-drop counter; negative at the start of a game until Down is first pressed.
        autorepeat_y: u8 = 0x0E,
        hold_down_points: u8 = 0x0F,
        lines: Bcd<2> = 0x10,
        row_y: u8 = 0x12,
        score: Bcd<3> = 0x13,
        completed_lines: u8 = 0x16,
        line_index: u8 = 0x17,
        curtain_row: u8 = 0x18,
        start_height: u8 = 0x19,
        garbage_hole: u8 = 0x1A,
        /// Unused, but copied with the rest of the block.
        unused_1b: [u8; 5] = 0x1B,
    }
}

ram_struct! {
    /// All of the game's named RAM.
    pub struct Vars {
        /// Scratch, shared by many routines.
        tmp1: u8 = 0x0000,
        tmp2: u8 = 0x0001,
        tmp3: u8 = 0x0002,
        tmp_bulk_copy_to_ppu_return_addr: Le16 = 0x0005,
        patch_to_ppu_addr: u8 = 0x0014,
        rng_seed: Be16 = 0x0017,
        spawn_id: u8 = 0x0019,
        spawn_count: u8 = 0x001A,
        /// Set by the NMI handler; the main loop clears it and waits for it.
        vertical_blanking_interval: u8 = 0x0033,
        unused_0e: u8 = 0x0034,
        active: Player = 0x0040,
        player1: Player = 0x0060,
        player2: Player = 0x0080,
        sprite_x_offset: u8 = 0x00A0,
        sprite_y_offset: u8 = 0x00A1,
        sprite_index_in_oam_content_lookup: u8 = 0x00A2,
        out_of_date_render_flags: u8 = 0x00A3,
        two_player_piece_delay_counter: u8 = 0x00A4,
        two_player_piece_delay_player: u8 = 0x00A5,
        two_player_piece_delay_piece: u8 = 0x00A6,
        game_mode_state: u8 = 0x00A7,
        general_counter: u8 = 0x00A8,
        general_counter2: u8 = 0x00A9,
        general_counter3: u8 = 0x00AA,
        general_counter4: u8 = 0x00AB,
        general_counter5: u8 = 0x00AC,
        selecting_level_or_height: u8 = 0x00AD,
        /// Saved Y (or X, or orientation) for undoing a move that turned out invalid.
        original_y: u8 = 0x00AE,
        drop_speed: u8 = 0x00AF,
        tmp_current_piece: u8 = 0x00B0,
        frame_counter: Le16 = 0x00B1,
        oam_staging_length: u8 = 0x00B3,
        newly_pressed_buttons: u8 = 0x00B5,
        held_buttons: u8 = 0x00B6,
        active_player: u8 = 0x00B7,
        playfield_addr: Le16 = 0x00B8,
        allegro: u8 = 0x00BA,
        pending_garbage: u8 = 0x00BB,
        pending_garbage_inactive_player: u8 = 0x00BC,
        render_mode: u8 = 0x00BD,
        number_of_players: u8 = 0x00BE,
        next_piece: u8 = 0x00BF,
        game_mode: u8 = 0x00C0,
        game_type: u8 = 0x00C1,
        music_type: u8 = 0x00C2,
        sleep_counter: u8 = 0x00C3,
        ending: u8 = 0x00C4,
        ending_custom_vars: [u8; 7] = 0x00C5,
        ending_current_sprite: u8 = 0x00CC,
        ending_type_b_cathedral_frame_delay_counter: u8 = 0x00CD,
        demo_held_buttons: u8 = 0x00CE,
        demo_repeats: u8 = 0x00CF,
        /// 0xFF while recording a demo (a development leftover: recorded input is written to
        /// the demo table's address, which is ROM).
        demo_recording: u8 = 0x00D0,
        /// Where the demo's next recorded input comes from (into `demoButtonsTable`).
        demo_buttons_addr: Le16 = 0x00D1,
        /// Index of the demo's next piece in `demoTetriminoTypeTable`.
        demo_index: u8 = 0x00D3,
        high_score_entry_name_offset_for_letter: u8 = 0x00D4,
        high_score_entry_raw_pos: u8 = 0x00D5,
        high_score_entry_name_offset_for_row: u8 = 0x00D6,
        high_score_entry_current_letter: u8 = 0x00D7,
        line_clear_stats_by_type: [u8; 4] = 0x00D8,
        total_score: Bcd<3> = 0x00DC,
        display_next_piece: u8 = 0x00DF,
        /// Sound engine scratch.
        audio_tmp: [u8; 5] = 0x00E0,
        music_chan_tmp_addr: Le16 = 0x00E6,
        music_unused2: u8 = 0x00EA,
        sound_rng_seed: Be16 = 0x00EB,
        current_sound_effect_slot: u8 = 0x00ED,
        music_channel_offset: u8 = 0x00EE,
        current_audio_slot: u8 = 0x00EF,
        unreferenced_button_mirror: [u8; 3] = 0x00F1,
        newly_pressed_buttons_player1: u8 = 0x00F5,
        newly_pressed_buttons_player2: u8 = 0x00F6,
        held_buttons_player1: u8 = 0x00F7,
        held_buttons_player2: u8 = 0x00F8,
        joy1_location: u8 = 0x00FB,
        ppu_scroll_y: u8 = 0x00FC,
        ppu_scroll_x: u8 = 0x00FD,
        current_ppu_mask: u8 = 0x00FE,
        current_ppu_ctrl: u8 = 0x00FF,
        /// Sprites staged for the next frame's OAM DMA.
        oam_staging: [u8; 256] = 0x0200,
        /// Pieces dealt per type (BCD, 2 bytes each), for the statistics panel.
        stats_by_type: [u8; 14] = 0x03F0,
        /// 20 rows of 10 cells, top row first: a block tile, or 0xEF (or above) for empty.
        /// The page's last 56 bytes are part of it too: the game fills them with empty tiles,
        /// and collision checks read them when a piece pokes above the top row (the 8-bit cell
        /// index wraps around).
        playfield: [u8; 256] = 0x0400,
        playfield_for_second_player: [u8; 256] = 0x0500,
        music_staging_sq1_lo: u8 = 0x0680,
        music_staging_sq1_hi: u8 = 0x0681,
        audio_initialized: u8 = 0x0682,
        music_pause_sound_effect_length_counter: u8 = 0x0683,
        music_staging_sq2_lo: u8 = 0x0684,
        music_staging_sq2_hi: u8 = 0x0685,
        music_staging_tri_lo: u8 = 0x0688,
        music_staging_tri_hi: u8 = 0x0689,
        reset_sq12_for_music: u8 = 0x068A,
        music_pause_sound_effect_counter: u8 = 0x068B,
        music_staging_noise_lo: u8 = 0x068C,
        music_staging_noise_hi: u8 = 0x068D,
        music_data_note_table_offset: u8 = 0x0690,
        music_data_duration_table_offset: u8 = 0x0691,
        music_data_chan_ptr: [u8; 8] = 0x0692,
        music_chan_control: [u8; 3] = 0x069A,
        music_chan_volume: [u8; 3] = 0x069D,
        music_data_chan_ptr_deref: [u8; 8] = 0x06A0,
        music_data_chan_ptr_off: [u8; 4] = 0x06A8,
        music_data_chan_instruction_offset: [u8; 4] = 0x06AC,
        music_data_chan_instruction_offset_backup: [u8; 4] = 0x06B0,
        music_chan_note_duration_remaining: [u8; 4] = 0x06B4,
        music_chan_note_duration: [u8; 4] = 0x06B8,
        music_chan_prog_loop_counter: [u8; 4] = 0x06BC,
        music_staging_sq1_sweep: [u8; 2] = 0x06C0,
        music_chan_note: [u8; 4] = 0x06C3,
        music_chan_inhibit: [u8; 3] = 0x06C8,
        music_track_dec: u8 = 0x06CC,
        music_chan_vol_frame_counter: [u8; 4] = 0x06CD,
        music_chan_lo_frame_counter: [u8; 4] = 0x06D1,
        /// Per sound-effect slot (0..=4 or 0..=3): frame counts, counters, scratch, requests.
        sfx_frame_count: [u8; 5] = 0x06D5,
        sfx_frame_counter: [u8; 5] = 0x06DA,
        sfx_secondary_counter: [u8; 4] = 0x06DF,
        sfx_tertiary_counter: [u8; 4] = 0x06E3,
        sfx_tmp: [u8; 4] = 0x06E7,
        /// A sound effect to start, per slot (0 = none); the sound engine consumes these.
        sfx_init: [u8; 5] = 0x06F0,
        music_track: u8 = 0x06F5,
        /// The sound effect playing, per slot (0 = none).
        sfx_playing: [u8; 5] = 0x06F8,
        currently_playing_music_track: u8 = 0x06FD,
        unreferenced_sound_rng_tmp: u8 = 0x06FF,
        high_score_names: [u8; 48] = 0x0700,
        high_score_scores_a: [u8; 12] = 0x0730,
        high_score_scores_b: [u8; 12] = 0x073C,
        high_score_levels: [u8; 8] = 0x0748,
        /// "Initialised" marker checked at reset to decide whether to clear the high scores.
        init_magic: [u8; 5] = 0x0750,
    }
}

/// Scratch the oracle doesn't compare by default: values left over in these after a routine
/// returns aren't part of its effect.
pub const SCRATCH: &[&str] = &["tmp1", "tmp2", "tmp3", "tmp_bulk_copy_to_ppu_return_addr", "audio_tmp"];

impl Vars {
    pub fn from_ram(ram: &[u8; 0x800]) -> Self {
        Self::load(ram)
    }

    pub fn to_ram(&self, ram: &mut [u8; 0x800]) {
        self.store(ram);
    }

    /// Operate on RAM by address, for the few routines that work on raw memory (page fills).
    /// Bytes no variable covers don't survive the round trip.
    pub fn with_ram<R>(&mut self, f: impl FnOnce(&mut [u8; 0x800]) -> R) -> R {
        let mut ram = [0u8; 0x800];
        self.store(&mut ram);
        let r = f(&mut ram);
        *self = Self::load(&ram);
        r
    }
}
