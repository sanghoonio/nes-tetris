//! The music engine: runs the current track's channel scripts once a frame.
//!
//! A track has four channels: square 1, square 2, triangle and noise (numbered 0..=3). Each
//! channel plays a *script list* from ROM: a list of addresses of scripts, ended by a 0 high byte
//! (stop the music) or a $FF high byte (the next two bytes are the address of another list to
//! continue with, so lists can loop). A script is a string of bytes:
//!
//! - `$00`: the script is over; go on to the next script of the list
//! - `$9F c v`: set the channel's control byte (envelope / pitch effect, see below) and volume
//! - `$9E d`: set the duration table offset;  `$9C n`: set the note offset
//! - `$C0 + n`: start a loop that runs its body `n` times; `$FF` ends the loop body
//! - `$B0 + d`: a duration (index into the duration table, plus the offset), followed by a note
//! - anything else: a note (for noise: an effect, see `play_noise` / `play_dmc`)
//!
//! A note is held for its duration, counted in frames. While it is held the engine animates it
//! every frame: the volume follows an envelope (`set_chan_vol`) and the pitch low byte follows a
//! small wobble (`set_chan_lo`), both chosen by the channel's control byte.
//!
//! Channel `ch` writes its registers at `SQ1 + 4 * ch`, which `music_channel_offset` holds while
//! the channels are visited in order.

use super::{NOISE, SQ1, SQ2};
use crate::shadow::{Ret, Routine};
use crate::{layout, Game};

const DMC_FREQ: u16 = 0x4010;
const DMC_RAW: u16 = 0x4011;
const DMC_START: u16 = 0x4012;
const DMC_LEN: u16 = 0x4013;
const SND_CHN: u16 = 0x4015;
/// Offsets of the registers within a channel's block.
const REG_VOL: u16 = 0;
const REG_SWEEP: u16 = 1;
const REG_LO: u16 = 2;
const REG_HI: u16 = 3;

/// Music track number meaning "stop all sound".
const TRACK_STOP_ALL: u8 = 0xFF;
/// The triangle channel, and the noise channel.
const TRI: u8 = 2;
const NOISE_CH: u8 = 3;
/// `music_chan_note` value for a rest-like silent note.
const NOTE_SILENT: u8 = 0x02;
/// Disables a square channel's sweep unit.
const SWEEP_OFF: u8 = 0x7F;
/// Square volume register value with the volume at 0 (length counter halted).
const VOLUME_OFF: u8 = 0x10;
/// A script-list entry whose high byte is this ends the list: continue with the list at the
/// address in the following two bytes (or, in the header, the channel is unused).
const LIST_LINK: u8 = 0xFF;
/// A script-list entry whose high byte is this stops the music.
const LIST_END: u8 = 0x00;
/// Number of bytes in a track's header in `musicDataTable`: note offset, duration offset and
/// four channel script-list pointers.
const TRACK_HEADER_LEN: u8 = 10;
/// Where the music's per-channel `musicChanControl` and `musicChanVolume` live. They sit next to
/// each other, so the noise channel (3) reaches into the volumes if its script sets them; the
/// accessors below address RAM directly to behave the same way.
const CHAN_CONTROL: u16 = 0x069A;
const CHAN_VOLUME: u16 = 0x069D;
/// `musicStagingSq1Sweep`: one byte per channel (the triangle's has no name).
const CHAN_SWEEP: u16 = 0x06C0;
/// An unnamed byte in the disassembly ($06F7, just before `sfx_playing`) that `play_dmc` checks:
/// the DMC is left alone while it is nonzero.
const DMC_BUSY: u16 = 0x06F7;

/// Control byte: low 5 bits pick a volume envelope (0 = none, constant volume).
const CONTROL_ENVELOPE: u8 = 0x1F;
/// Control byte: high 3 bits pick the pitch wobble (0 = none).
const CONTROL_PITCH_EFFECT: u8 = 0xE0;

/// Script bytes.
const OP_SET_CONTROL_AND_VOLUME: u8 = 0x9F;
const OP_SET_DURATION_OFFSET: u8 = 0x9E;
const OP_SET_NOTE_OFFSET: u8 = 0x9C;
const OP_LOOP_END: u8 = 0xFF;
/// `$C0..=$FF` start a loop (but `$FF` ends one); the low 6 bits are the repeat count.
const OP_LOOP_START_MASK: u8 = 0xC0;
const LOOP_COUNT_MASK: u8 = 0x3F;
/// `$B0..=$BF` is a duration; the low nibble indexes the duration table.
const OP_DURATION_MASK: u8 = 0xB0;
const DURATION_INDEX_MASK: u8 = 0x0F;

/// How a pass over one channel ended.
enum ChannelPass {
    /// Go on to the next channel.
    Done,
    /// The channel got a new script; run the channel again from the top.
    Rerun,
    /// The music ended (the whole frame's work is over).
    MusicEnded,
}

impl Game {
    // ---- raw RAM access for the arrays that alias ----

    fn ram_byte(&mut self, addr: u16) -> u8 {
        self.vars.with_ram(|ram| ram[addr as usize])
    }

    fn set_ram_byte(&mut self, addr: u16, v: u8) {
        self.vars.with_ram(|ram| ram[addr as usize] = v);
    }

    fn chan_control(&mut self, ch: u8) -> u8 {
        self.ram_byte(CHAN_CONTROL + ch as u16)
    }

    fn chan_volume(&mut self, ch: u8) -> u8 {
        self.ram_byte(CHAN_VOLUME + ch as u16)
    }

    /// The staged low byte of the pitch of the channel whose registers start `offset` bytes from
    /// `SQ1` (0, 4 or 8): `musicStagingSq1Lo,offset`.
    fn staged_lo(&self, offset: u8) -> u8 {
        match offset {
            0 => self.vars.music_staging_sq1_lo,
            4 => self.vars.music_staging_sq2_lo,
            _ => self.vars.music_staging_tri_lo,
        }
    }

    fn staged_hi(&self, offset: u8) -> u8 {
        match offset {
            0 => self.vars.music_staging_sq1_hi,
            4 => self.vars.music_staging_sq2_hi,
            _ => self.vars.music_staging_tri_hi,
        }
    }

    fn stage_pitch(&mut self, offset: u8, lo: u8, hi: u8) {
        match offset {
            0 => {
                self.vars.music_staging_sq1_lo = lo;
                self.vars.music_staging_sq1_hi = hi;
            }
            4 => {
                self.vars.music_staging_sq2_lo = lo;
                self.vars.music_staging_sq2_hi = hi;
            }
            _ => {
                self.vars.music_staging_tri_lo = lo;
                self.vars.music_staging_tri_hi = hi;
            }
        }
    }

    /// Whether a sound effect currently has channel `ch`'s hardware (`musicChanInhibit,x`).
    fn chan_inhibited(&self, ch: u8) -> bool {
        self.vars.channel_inhibit[ch as usize + 1] != 0
    }

    // ---- the frame ----

    /// `updateMusic`: once a frame. A new track request (`music_track`) loads that track and
    /// plays its first frame; track $FF silences everything; with no request the current track
    /// (if any) plays its next frame.
    pub fn update_music(&mut self) {
        let track = self.vars.music_track;
        if track == TRACK_STOP_ALL {
            self.sound_effect_slot2_makes_no_sound();
            return;
        }
        if track == 0 {
            if self.vars.currently_playing_music_track != 0 {
                self.update_music_frame();
            }
            return;
        }
        self.vars.current_audio_slot = track;
        // Tracks are numbered from 1; the tables start at 0.
        self.vars.music_track_dec = track.wrapping_sub(1);
        self.vars.music_staging_sq1_sweep = [SWEEP_OFF; 2];
        self.load_music_track();
        self.update_music_frame();
    }

    /// `muteAudioAndClearTriControl`: silence the APU and forget the triangle's control byte.
    fn mute_audio_and_clear_tri_control(&mut self) {
        self.mute_audio();
        self.apu_write(DMC_RAW, 0);
        self.vars.music_chan_control[TRI as usize] = 0;
    }

    /// `loadMusicTrack`: start the track `current_audio_slot` (its 0-based number is in
    /// `music_track_dec`): copy its header into RAM, point every channel at its first script
    /// and make every channel start a new note on the coming frame.
    pub fn load_music_track(&mut self) {
        self.mute_audio_and_clear_tri_control();
        self.vars.currently_playing_music_track = self.vars.current_audio_slot;
        let header = self.rom.byte(layout::MUSIC_DATA_TABLE_INDEX + self.vars.music_track_dec as u16);
        // The 10-byte header lands in `music_data_note_table_offset`, `..duration_table_offset`
        // and the four script-list pointers, which are contiguous in RAM.
        for i in 0..TRACK_HEADER_LEN {
            let v = self.rom.byte(layout::MUSIC_DATA_TABLE + header.wrapping_add(i) as u16);
            match i {
                0 => self.vars.music_data_note_table_offset = v,
                1 => self.vars.music_data_duration_table_offset = v,
                _ => self.vars.music_data_chan_ptr[i as usize - 2] = v,
            }
        }
        self.vars.music_chan_note_duration_remaining = [1; 4];
        self.vars.music_unused2 = 0;
        self.vars.music_data_chan_ptr_off = [0; 4];
        self.vars.music_data_chan_instruction_offset = [0; 4];
        // Each channel starts at the first script of its list.
        for ch in 0..4usize {
            let lo = self.vars.music_data_chan_ptr[ch * 2];
            let hi = self.vars.music_data_chan_ptr[ch * 2 + 1];
            self.vars.music_chan_tmp_addr.0 = (self.vars.music_chan_tmp_addr.0 & 0xFF00) | lo as u16;
            if hi == LIST_LINK {
                // Unused channel.
                self.vars.music_data_chan_ptr_deref[ch * 2] = 0xFF;
                self.vars.music_data_chan_ptr_deref[ch * 2 + 1] = 0xFF;
                continue;
            }
            self.vars.music_chan_tmp_addr.0 = (self.vars.music_chan_tmp_addr.0 & 0x00FF) | (hi as u16) << 8;
            // The offset into channel 0's list: always 0 here (just cleared).
            let off = self.vars.music_data_chan_ptr_off[0];
            let list = self.vars.music_chan_tmp_addr.0;
            self.vars.music_data_chan_ptr_deref[ch * 2] = self.rom.byte(list.wrapping_add(off as u16));
            self.vars.music_data_chan_ptr_deref[ch * 2 + 1] = self.rom.byte(list.wrapping_add(off.wrapping_add(1) as u16));
        }
    }

    /// `initSq12IfTrashedBySoundEffect`: after a sound effect used the square channels, put the
    /// music's staged pitch back into them. `reset_sq12_for_music` is 1 for square 1 only, any
    /// other nonzero value for both.
    pub fn init_sq12_if_trashed_by_sound_effect(&mut self) {
        let reset = self.vars.reset_sq12_for_music;
        if reset == 0 {
            return;
        }
        if reset != 1 {
            self.apu_write(SQ2 + REG_SWEEP, SWEEP_OFF);
            self.apu_write(SQ2 + REG_LO, self.vars.music_staging_sq2_lo);
            self.apu_write(SQ2 + REG_HI, self.vars.music_staging_sq2_hi);
        }
        self.apu_write(SQ1 + REG_SWEEP, SWEEP_OFF);
        self.apu_write(SQ1 + REG_LO, self.vars.music_staging_sq1_lo);
        self.apu_write(SQ1 + REG_HI, self.vars.music_staging_sq1_hi);
        self.vars.reset_sq12_for_music = 0;
    }

    /// `updateMusicFrame`: advance every channel by one frame.
    fn update_music_frame(&mut self) {
        self.init_sq12_if_trashed_by_sound_effect();
        self.vars.music_channel_offset = 0;
        for ch in 0..4u8 {
            if ch != 0 {
                self.vars.music_channel_offset = self.vars.music_channel_offset.wrapping_add(4);
            }
            loop {
                match self.update_channel(ch) {
                    ChannelPass::Done => break,
                    ChannelPass::Rerun => continue,
                    ChannelPass::MusicEnded => return,
                }
            }
        }
    }

    /// `updateMusicFrame_updateChannel`: if the channel's current note is still being held,
    /// animate it; otherwise run its script until it starts the next note.
    fn update_channel(&mut self, ch: u8) -> ChannelPass {
        let c = ch as usize;
        let script = u16::from_le_bytes([
            self.vars.music_data_chan_ptr_deref[c * 2],
            self.vars.music_data_chan_ptr_deref[c * 2 + 1],
        ]);
        self.vars.music_chan_tmp_addr.0 = script;
        if script >> 8 == LIST_LINK as u16 {
            // Unused channel.
            return ChannelPass::Done;
        }
        self.vars.music_chan_note_duration_remaining[c] = self.vars.music_chan_note_duration_remaining[c].wrapping_sub(1);
        if self.vars.music_chan_note_duration_remaining[c] != 0 {
            self.set_chan_vol(ch);
            self.set_chan_lo(ch);
            return ChannelPass::Done;
        }
        self.vars.music_chan_vol_frame_counter[c] = 0;
        self.vars.music_chan_lo_frame_counter[c] = 0;
        self.run_script(ch)
    }

    /// Read script bytes of channel `ch` until a note starts (or the script ends).
    fn run_script(&mut self, ch: u8) -> ChannelPass {
        let c = ch as usize;
        loop {
            let op = self.music_get_next_instruction_byte(ch);
            if op == 0 {
                return self.next_script(ch);
            }
            match op {
                OP_SET_CONTROL_AND_VOLUME => {
                    let control = self.music_get_next_instruction_byte(ch);
                    self.set_ram_byte(CHAN_CONTROL + ch as u16, control);
                    let volume = self.music_get_next_instruction_byte(ch);
                    self.set_ram_byte(CHAN_VOLUME + ch as u16, volume);
                    continue;
                }
                OP_SET_DURATION_OFFSET => {
                    self.vars.music_data_duration_table_offset = self.music_get_next_instruction_byte(ch);
                    continue;
                }
                OP_SET_NOTE_OFFSET => {
                    self.vars.music_data_note_table_offset = self.music_get_next_instruction_byte(ch);
                    continue;
                }
                _ => {}
            }
            let mut starts_loop = op & OP_LOOP_START_MASK == OP_LOOP_START_MASK;
            if op == OP_LOOP_END {
                if self.vars.music_chan_prog_loop_counter[c] == 0 {
                    continue;
                }
                self.vars.music_chan_prog_loop_counter[c] -= 1;
                let back = self.vars.music_data_chan_instruction_offset_backup[c];
                self.vars.music_data_chan_instruction_offset[c] = back;
                if back != 0 {
                    continue;
                }
                // A loop body that starts at offset 0 can't be re-entered: the original then
                // falls into the code that starts a loop, with the $FF byte as its count.
                starts_loop = true;
            }
            if starts_loop {
                self.vars.music_chan_prog_loop_counter[c] = (op & LOOP_COUNT_MASK).wrapping_sub(1);
                self.vars.music_data_chan_instruction_offset_backup[c] = self.vars.music_data_chan_instruction_offset[c];
                continue;
            }
            return self.start_note(ch, op);
        }
    }

    /// A note byte (optionally a duration first) was read: start the note.
    fn start_note(&mut self, ch: u8, op: u8) -> ChannelPass {
        let c = ch as usize;
        let mut note = op;
        if op & OP_DURATION_MASK == OP_DURATION_MASK {
            let index = (op & DURATION_INDEX_MASK).wrapping_add(self.vars.music_data_duration_table_offset);
            let duration = self.rom.byte(layout::NOTE_DURATION_TABLE + index as u16);
            self.vars.music_chan_note_duration[c] = duration;
            if ch == TRI {
                let control = self.chan_control(TRI);
                self.set_ram_byte(CHAN_VOLUME + TRI as u16, tri_linear_counter(control, duration));
            }
            note = self.music_get_next_instruction_byte(ch);
        }
        self.vars.music_chan_note[c] = note;
        if ch == NOISE_CH {
            self.play_dmc(note);
            self.play_noise(note & 0x3F);
            self.vars.music_chan_note_duration_remaining[c] = self.vars.music_chan_note_duration[c];
            return ChannelPass::Done;
        }

        let offset = self.vars.music_channel_offset;
        // Pitch of the note: [hi, lo] pairs per note number; a lo of 0 marks a rest.
        let lo_of = |g: &Game, n: u8| g.rom.byte(layout::NOTE_TO_WAVE_TABLE + n as u16 + 1);
        let hi_of = |g: &Game, n: u8| g.rom.byte(layout::NOTE_TO_WAVE_TABLE + n as u16);
        let is_rest = lo_of(self, note) == 0;
        if !is_rest {
            let shift = self.vars.music_data_note_table_offset;
            // The note offset is sign-and-magnitude. Negative: the original's `clc; sbc` takes
            // one extra off.
            let shifted = if shift & 0x80 != 0 {
                note.wrapping_sub(shift & 0x7F).wrapping_sub(1)
            } else {
                note.wrapping_add(shift)
            };
            let lo = lo_of(self, shifted);
            let hi = hi_of(self, shifted) | 0x08;
            self.stage_pitch(offset, lo, hi);
        }
        let volume = if !is_rest {
            self.chan_volume(ch)
        } else if ch == TRI {
            0
        } else {
            VOLUME_OFF
        };
        if !self.chan_inhibited_by_own_slot(ch) {
            // With an envelope the volume register is set by `set_chan_vol`; start it at the
            // nibble's high half only (or "off").
            let uses_envelope = ch != TRI && self.chan_control(ch) & CONTROL_ENVELOPE != 0;
            let vol = if uses_envelope && volume != VOLUME_OFF && volume & 0xF0 != 0 { volume & 0xF0 } else { volume };
            self.apu_write(SQ1 + offset as u16 + REG_VOL, vol);
            let sweep = self.ram_byte(CHAN_SWEEP + ch as u16);
            self.apu_write(SQ1 + offset as u16 + REG_SWEEP, sweep);
            self.apu_write(SQ1 + offset as u16 + REG_LO, self.staged_lo(offset));
            self.apu_write(SQ1 + offset as u16 + REG_HI, self.staged_hi(offset));
        }
        self.vars.music_chan_note_duration_remaining[c] = self.vars.music_chan_note_duration[c];
        ChannelPass::Done
    }

    /// True while the sound effect in the slot with the same number as the channel has it:
    /// `channel_inhibit[ch + 1] == ch + 1`. (This is a stricter test than `chan_inhibited`'s
    /// "nonzero": the original compares against the channel number.)
    fn chan_inhibited_by_own_slot(&self, ch: u8) -> bool {
        self.vars.channel_inhibit[ch as usize + 1].wrapping_sub(1) == ch
    }

    /// The script ended (`$00`): go on to the channel's next script, following list links.
    fn next_script(&mut self, ch: u8) -> ChannelPass {
        let c = ch as usize;
        let list = u16::from_le_bytes([self.vars.music_data_chan_ptr[c * 2], self.vars.music_data_chan_ptr[c * 2 + 1]]);
        self.vars.music_chan_tmp_addr.0 = list;
        self.vars.music_data_chan_ptr_off[c] = self.vars.music_data_chan_ptr_off[c].wrapping_add(2);
        let mut at = self.vars.music_data_chan_ptr_off[c];
        loop {
            let lo = self.rom.byte(self.vars.music_chan_tmp_addr.0.wrapping_add(at as u16));
            at = at.wrapping_add(1);
            let hi = self.rom.byte(self.vars.music_chan_tmp_addr.0.wrapping_add(at as u16));
            self.vars.music_data_chan_ptr_deref[c * 2] = lo;
            self.vars.music_data_chan_ptr_deref[c * 2 + 1] = hi;
            if hi == LIST_END {
                self.sound_effect_slot2_makes_no_sound();
                return ChannelPass::MusicEnded;
            }
            if hi != LIST_LINK {
                self.vars.music_data_chan_instruction_offset[c] = 0;
                self.vars.music_chan_note_duration_remaining[c] = 1;
                return ChannelPass::Rerun;
            }
            // A link: the two bytes after it are the address of the list to loop to.
            at = at.wrapping_add(1);
            let tmp = self.vars.music_chan_tmp_addr.0;
            let new_lo = self.rom.byte(tmp.wrapping_add(at as u16));
            at = at.wrapping_add(1);
            let new_hi = self.rom.byte(tmp.wrapping_add(at as u16));
            self.vars.music_data_chan_ptr[c * 2] = new_lo;
            self.vars.music_data_chan_ptr[c * 2 + 1] = new_hi;
            self.vars.music_chan_tmp_addr.0 = u16::from_le_bytes([new_lo, new_hi]);
            self.vars.music_data_chan_ptr_off[c] = 0;
            at = 0;
        }
    }

    /// `musicGetNextInstructionByte`: the next byte of channel `ch`'s current script.
    pub fn music_get_next_instruction_byte(&mut self, ch: u8) -> u8 {
        let c = ch as usize;
        let at = self.vars.music_data_chan_instruction_offset[c];
        self.vars.music_data_chan_instruction_offset[c] = at.wrapping_add(1);
        self.rom.byte(self.vars.music_chan_tmp_addr.0.wrapping_add(at as u16))
    }

    // ---- per-frame animation of a held note ----

    /// `updateMusicFrame_setChanVol`: move a square channel's volume along its envelope. The low
    /// 5 bits of the control byte pick the envelope: a ROM list of volume nibbles, high nibble
    /// first, one per frame. `$FF` ends it holding the channel's own volume, `$F0` ends it
    /// muted. Does nothing for the triangle and noise.
    pub fn set_chan_vol(&mut self, ch: u8) {
        if ch >= TRI {
            return;
        }
        let c = ch as usize;
        let envelope = self.chan_control(ch) & CONTROL_ENVELOPE;
        if envelope == 0 {
            return;
        }
        if self.vars.music_chan_note[c] == NOTE_SILENT {
            // (Only meaningful for noise.) Mute, and keep counting.
            self.vars.music_chan_vol_frame_counter[c] = self.vars.music_chan_vol_frame_counter[c].wrapping_add(1);
            self.write_chan_vol(ch, VOLUME_OFF);
            return;
        }
        let list = self.rom.word(layout::MUSIC_CHAN_VOL_CONTROL_TABLE + (envelope as u16 - 1) * 2);
        let frame = self.vars.music_chan_vol_frame_counter[c];
        let entry = self.rom.byte(list.wrapping_add((frame >> 1) as u16));
        if entry == 0xFF {
            // Hold the channel's volume (muted if it is 0).
            let v = self.chan_volume(ch);
            self.write_chan_vol(ch, if v != 0 { v } else { VOLUME_OFF });
        } else if entry == 0xF0 {
            self.write_chan_vol(ch, VOLUME_OFF);
        } else {
            let nibble = if frame & 1 == 0 { entry >> 4 } else { entry & 0x0F };
            let v = (self.chan_volume(ch) & 0xF0) | nibble;
            self.vars.music_chan_vol_frame_counter[c] = frame.wrapping_add(1);
            self.write_chan_vol(ch, v);
        }
    }

    fn write_chan_vol(&mut self, ch: u8, v: u8) {
        if self.chan_inhibited(ch) {
            return;
        }
        let offset = self.vars.music_channel_offset;
        self.apu_write(SQ1 + offset as u16 + REG_VOL, v);
    }

    /// `updateMusicFrame_setChanLo`: apply the pitch wobble chosen by the top 3 bits of the
    /// control byte to a held note. Not on the noise channel, and not in track 1.
    pub fn set_chan_lo(&mut self, ch: u8) {
        if self.vars.currently_playing_music_track == 1 || ch == NOISE_CH {
            return;
        }
        let c = ch as usize;
        let effect = self.chan_control(ch) & CONTROL_PITCH_EFFECT;
        if effect == 0 {
            return;
        }
        self.vars.audio_tmp[0] = effect;
        if self.vars.music_chan_note[c] != NOTE_SILENT {
            let offset = self.vars.music_channel_offset;
            self.vars.audio_tmp[1] = self.staged_lo(offset);
            self.set_chan_lo_offset(ch);
        }
        self.vars.music_chan_lo_frame_counter[c] = self.vars.music_chan_lo_frame_counter[c].wrapping_add(1);
    }

    /// `updateMusicFrame_setChanLoOffset`: from `audio_tmp[0]` (the pitch effect) and
    /// `audio_tmp[1]` (the note's low byte), write this frame's low byte to the channel.
    /// Each effect is a table of small signed offsets indexed by frames since the note began.
    pub fn set_chan_lo_offset(&mut self, ch: u8) {
        let c = ch as usize;
        let effect = self.vars.audio_tmp[0];
        let base = self.vars.audio_tmp[1];
        let frame = self.vars.music_chan_lo_frame_counter[c];
        match effect {
            // Slow start, then trills; the last part loops through the trill table.
            0xA0 => {
                let i = if frame == 0x2B { 0x21 } else { frame };
                let off = self.rom.byte(layout::LO_OFF_SLOW_START_TRILL_TABLE + i as u16);
                self.set_lo_and_save_frame(ch, i, off, base);
            }
            // The same offset every frame.
            0x60 => self.set_lo(ch, 0xFE, base),
            // Doesn't loop: falls to -11, bounces, then holds -10.
            0x40 => {
                let off = if frame < 0x10 {
                    self.rom.byte(layout::LO_OFF_DESCEND_TO_NEG11_BOUNCE_TO_NEG9_TABLE + frame as u16)
                } else {
                    0xF6
                };
                self.set_lo(ch, off, base);
            }
            // Falls from +9 to 0 and holds (the note $46 holds 0).
            0x80 | 0xC0 => {
                let i = if frame == 0x31 { 0x27 } else { frame };
                let mut off = self.rom.byte(layout::LO_OFF9_TO0_FALL_TABLE + i as u16);
                if self.vars.music_chan_note[c] == 0x46 {
                    off = 0;
                }
                self.set_lo_and_save_frame(ch, i, off, base);
            }
            // $20 and $E0: a trill, looping every 10 frames.
            _ => {
                let i = if frame == 0x0A { 0 } else { frame };
                let off = self.rom.byte(layout::LO_OFF_TRILL_NEG2_TO2_TABLE + i as u16);
                self.set_lo_and_save_frame(ch, i, off, base);
            }
        }
    }

    /// `musicLoOffset_setLoAndSaveFrameCounter`: also rewind/keep the effect's frame counter.
    fn set_lo_and_save_frame(&mut self, ch: u8, frame: u8, off: u8, base: u8) {
        self.vars.music_chan_lo_frame_counter[ch as usize] = frame;
        self.set_lo(ch, off, base);
    }

    /// `musicLoOffset_setLo`: write `base + off` (8-bit) to the channel's low byte register
    /// unless a sound effect has the channel.
    fn set_lo(&mut self, ch: u8, off: u8, base: u8) {
        if self.chan_inhibited(ch) {
            return;
        }
        let offset = self.vars.music_channel_offset;
        self.apu_write(SQ1 + offset as u16 + REG_LO, off.wrapping_add(base));
    }

    // ---- noise channel ----

    /// `playNoise`: start the noise effect at `index` in `noises_table` (3 register bytes per
    /// effect), unless slot 0's sound effect (game over, rocket) is playing.
    pub fn play_noise(&mut self, index: u8) {
        if self.vars.sfx_playing[0] != 0 {
            return;
        }
        let at = layout::NOISES_TABLE + index as u16;
        self.apu_write(NOISE + REG_VOL, self.rom.byte(at));
        self.apu_write(NOISE + REG_LO, self.rom.byte(at + 1));
        self.apu_write(NOISE + REG_HI, self.rom.byte(at + 2));
    }

    /// `playDmc`: the top 2 bits of a noise-channel byte start a DMC sample: 01 the first
    /// (short) one, 10 the second; 00 and 11 none. Not while `DMC_BUSY` is nonzero.
    pub fn play_dmc(&mut self, op: u8) {
        let (len, start) = match op & 0xC0 {
            0x40 => (0x07, 0x00),
            0x80 => (0x0F, 0x02),
            _ => return,
        };
        let freq = 0x0E;
        self.apu_write(DMC_LEN, len);
        self.apu_write(DMC_START, start);
        if self.ram_byte(DMC_BUSY) != 0 {
            return;
        }
        self.apu_write(DMC_FREQ, freq);
        // Enable the four tone channels only, zero the DMC output level, then enable the DMC
        // too (which starts the sample).
        self.apu_write(SND_CHN, 0x0F);
        self.apu_write(DMC_RAW, 0);
        self.apu_write(SND_CHN, 0x1F);
    }
}

/// The triangle's "volume" is its linear counter reload value. A control byte with an envelope
/// number uses that number; otherwise the note's duration decides: (duration - 1) * 4 quarter
/// frames, at most 60. Control bits 7-6 of `01`/`10` (and a zero duration) mean "full" ($FF).
fn tri_linear_counter(control: u8, duration: u8) -> u8 {
    if control & CONTROL_ENVELOPE != 0 {
        return control & CONTROL_ENVELOPE;
    }
    let high = control & 0xC0;
    if (high == 0 || high == 0xC0) && duration != 0 {
        let quarter_frames = duration.wrapping_add(0xFF) << 2;
        quarter_frames.min(0x3C)
    } else {
        0xFF
    }
}

pub const SHADOW: &[Routine] = &[
    Routine {
        name: "updateMusic",
        addr: layout::UPDATE_MUSIC,
        run: |g, _| {
            g.update_music();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "loadMusicTrack",
        addr: layout::LOAD_MUSIC_TRACK,
        run: |g, _| {
            g.load_music_track();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "updateMusicFrame_setChanVol",
        addr: layout::UPDATE_MUSIC_FRAME_SET_CHAN_VOL,
        run: |g, r| {
            g.set_chan_vol(r.x);
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "updateMusicFrame_setChanLo",
        addr: layout::UPDATE_MUSIC_FRAME_SET_CHAN_LO,
        run: |g, r| {
            g.set_chan_lo(r.x);
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "updateMusicFrame_setChanLoOffset",
        addr: layout::UPDATE_MUSIC_FRAME_SET_CHAN_LO_OFFSET,
        run: |g, r| {
            g.set_chan_lo_offset(r.x);
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "initSq12IfTrashedBySoundEffect",
        addr: layout::INIT_SQ12_IF_TRASHED_BY_SOUND_EFFECT,
        run: |g, _| {
            g.init_sq12_if_trashed_by_sound_effect();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "playNoise",
        addr: layout::PLAY_NOISE,
        run: |g, r| {
            g.play_noise(r.y);
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "playDmc",
        addr: layout::PLAY_DMC,
        run: |g, r| {
            g.play_dmc(r.y);
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "musicGetNextInstructionByte",
        addr: layout::MUSIC_GET_NEXT_INSTRUCTION_BYTE,
        run: |g, r| Ret::A(g.music_get_next_instruction_byte(r.x)),
        scratch: &[],
    },
];
