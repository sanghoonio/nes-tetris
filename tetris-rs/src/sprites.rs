//! Staging sprites for the falling piece and the next-piece preview.

use crate::shadow::{Ret, Routine};
use crate::{layout, Game};

/// Attribute byte for the piece's minos: palette 2, in front of the background.
const PALETTE_FRONT: u8 = 0x02;
/// Tile that hides a mino.
const TILE_HIDDEN: u8 = 0xFF;
/// The smallest y the screen shows. Minos above it are hidden (the check is against the screen,
/// not the playfield).
const SCREEN_TOP_Y: u8 = 0x2F;
/// Screen x of the falling piece's centre block, before the carry quirk in
/// `stage_sprite_for_current_piece`.
const CENTRE_X_BASE: u8 = 0x60;
/// Two-player offsets: player 1's field is further left, player 2's further right.
const TWO_PLAYER_LEFT: u8 = 0x40;
const TWO_PLAYER_RIGHT: u8 = 0x6F;
/// Screen y of the falling piece's centre block.
const CENTRE_Y_BASE: u8 = 0x2F;
/// Where the next-piece preview is drawn.
const NEXT_PIECE_X: u8 = 0xC8;
const NEXT_PIECE_Y: u8 = 0x77;
/// Sentinel that ends a sprite's entries in the OAM content tables.
const SPRITE_END: u8 = 0xFF;

/// 6502 `adc`: the sum and the carry out.
fn adc(a: u8, b: u8, carry: bool) -> (u8, bool) {
    let sum = a as u16 + b as u16 + carry as u16;
    (sum as u8, sum > 0xFF)
}

/// 6502 `rol`: shift left through the carry.
fn rol(a: u8, carry: bool) -> (u8, bool) {
    (a << 1 | carry as u8, a & 0x80 != 0)
}

impl Game {
    /// `stageSpriteForCurrentPiece`: stage the four minos of the falling piece as sprites at its
    /// position on screen. A mino above the screen is hidden, with its x coordinate zeroed.
    pub fn stage_sprite_for_current_piece(&mut self) {
        let column = self.vars.active.tetrimino_x;
        // Column * 8 + CENTRE_X_BASE, with the carry out of the third shift (bit 5 of the
        // column) added in.
        let (x, _) = adc(column.wrapping_shl(3), CENTRE_X_BASE, column & 0x20 != 0);
        let centre_x = if self.vars.number_of_players == 1 {
            x
        } else {
            let x = x.wrapping_sub(TWO_PLAYER_LEFT);
            if self.vars.active_player == 1 {
                x
            } else {
                // The carry is left over from comparing activePlayer with 1.
                adc(x, TWO_PLAYER_RIGHT, self.vars.active_player >= 1).0
            }
        };

        // Row * 8 with the carry out of the third shift (bit 5 of the row) added in.
        let (r1, c1) = rol(self.vars.active.tetrimino_y, false);
        let (r2, c2) = rol(r1, c1);
        let (r3, c3) = rol(r2, c2);
        let centre_y = adc(r3, CENTRE_Y_BASE, c3).0;

        // Index into the orientation table: 12 bytes per orientation. Computed as the original
        // does (shift chain with carries), which is 12 * piece for every piece in use.
        let piece = self.vars.active.current_piece;
        let (p1, d1) = rol(piece, false);
        let (p2, d2) = rol(p1, d1);
        let (p3, d3) = rol(p2, d2);
        let mut table_index = adc(p3, p2, d3).0;

        // The original works in generalCounter..5 and originalY, and what it leaves there is
        // visible: the B-type ending draws generalCounter4/5 as its bonus digits, and on its last
        // frame they hold this routine's leftovers. So they're kept as the original keeps them.
        self.vars.general_counter3 = centre_x;
        self.vars.general_counter4 = centre_y;
        self.vars.general_counter5 = piece;
        self.vars.general_counter = p2;
        self.vars.general_counter2 = 0;

        let mut oam = self.vars.oam_staging_length;
        for _ in 0..4 {
            let mino_y = self.rom.table(layout::ORIENTATION_TABLE, table_index as usize).wrapping_shl(3).wrapping_add(centre_y);
            let tile = self.rom.table(layout::ORIENTATION_TABLE, table_index.wrapping_add(1) as usize);
            let mino_x_offset = self.rom.table(layout::ORIENTATION_TABLE, table_index.wrapping_add(2) as usize);
            self.vars.original_y = mino_y;
            let staged = &mut self.vars.oam_staging;
            staged[oam as usize] = mino_y;
            staged[oam.wrapping_add(1) as usize] = tile;
            staged[oam.wrapping_add(2) as usize] = PALETTE_FRONT;
            if mino_y < SCREEN_TOP_Y {
                staged[oam.wrapping_add(1) as usize] = TILE_HIDDEN;
                staged[oam.wrapping_add(3) as usize] = 0;
            } else {
                staged[oam.wrapping_add(3) as usize] = mino_x_offset.wrapping_shl(3).wrapping_add(centre_x);
            }
            // Each mino takes four bytes of OAM.
            oam = oam.wrapping_add(4);
            table_index = table_index.wrapping_add(3);
        }
        self.vars.oam_staging_length = oam;
    }

    /// `stageSpriteForNextPiece`: draw the next piece in the preview, unless the preview is
    /// switched off.
    pub fn stage_sprite_for_next_piece(&mut self) {
        if self.vars.display_next_piece != 0 {
            return;
        }
        self.vars.sprite_x_offset = NEXT_PIECE_X;
        self.vars.sprite_y_offset = NEXT_PIECE_Y;
        let next = self.vars.next_piece;
        self.vars.sprite_index_in_oam_content_lookup = self.rom.table(layout::ORIENTATION_TO_SPRITE_TABLE, next as usize);
        self.load_sprite_into_oam_staging();
    }

    /// `loadSpriteIntoOamStaging`: stage the sprite named by `sprite_index_in_oam_content_lookup`
    /// at the sprite offsets. Its entries in the content table are (y, tile, attributes, x), each
    /// relative to the offsets, until a $FF.
    pub fn load_sprite_into_oam_staging(&mut self) {
        // Pointer to the sprite's entries, from the two-byte table at oamContentLookup.
        let index = self.vars.sprite_index_in_oam_content_lookup.wrapping_shl(1);
        let lo = self.rom.table(layout::OAM_CONTENT_LOOKUP, index as usize);
        let hi = self.rom.table(layout::OAM_CONTENT_LOOKUP, index.wrapping_add(1) as usize);
        let entries = u16::from_le_bytes([lo, hi]);

        let mut oam = self.vars.oam_staging_length;
        let mut i: u8 = 0;
        loop {
            let at = |i: u8| self.rom.byte(entries.wrapping_add(i as u16));
            if at(i) == SPRITE_END {
                break;
            }
            let y = at(i).wrapping_add(self.vars.sprite_y_offset);
            let tile = at(i.wrapping_add(1));
            let attributes = at(i.wrapping_add(2));
            let x = at(i.wrapping_add(3)).wrapping_add(self.vars.sprite_x_offset);
            let staged = &mut self.vars.oam_staging;
            staged[oam as usize] = y;
            staged[oam.wrapping_add(1) as usize] = tile;
            staged[oam.wrapping_add(2) as usize] = attributes;
            staged[oam.wrapping_add(3) as usize] = x;
            oam = oam.wrapping_add(4);
            i = i.wrapping_add(4);
            self.vars.oam_staging_length = oam;
        }
    }
}

/// Pointer and loop counters for the sprite's entries; the routine never reads them afterwards.
const LOAD_SPRITE_SCRATCH: &[&str] = &["general_counter", "general_counter2"];

pub const SHADOW: &[Routine] = &[
    Routine {
        name: "stageSpriteForCurrentPiece",
        addr: layout::STAGE_SPRITE_FOR_CURRENT_PIECE,
        run: |g, _| {
            g.stage_sprite_for_current_piece();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "stageSpriteForNextPiece",
        addr: layout::STAGE_SPRITE_FOR_NEXT_PIECE,
        run: |g, _| {
            g.stage_sprite_for_next_piece();
            Ret::Unit
        },
        scratch: LOAD_SPRITE_SCRATCH,
    },
    Routine {
        name: "loadSpriteIntoOamStaging",
        addr: layout::LOAD_SPRITE_INTO_OAM_STAGING,
        run: |g, _| {
            g.load_sprite_into_oam_staging();
            Ret::Unit
        },
        scratch: LOAD_SPRITE_SCRATCH,
    },
];
