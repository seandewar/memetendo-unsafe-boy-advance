use intbits::Bits;

use crate::arbitrary_sign_extend;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Mode {
    Tile,
    Bitmap,
    Invalid,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Default, Copy, Clone, Debug)]
pub struct DisplayControl {
    mode: u8,
    frame_select: u8,
    pub hblank_oam_access: bool,
    pub obj_1d: bool,
    pub forced_blank: bool,

    pub display_bg: [bool; 4],
    pub display_obj: bool,
    pub display_bg_window: [bool; 2],
    pub display_obj_window: bool,
}

impl DisplayControl {
    pub fn lo_bits(&self) -> u8 {
        let mut bits = 0;
        bits.set_bits(..3, self.mode.bits(..3));
        bits.set_bit(4, self.frame_select.bit(0));
        bits.set_bit(5, self.hblank_oam_access);
        bits.set_bit(6, self.obj_1d);
        bits.set_bit(7, self.forced_blank);

        bits
    }

    pub fn hi_bits(&self) -> u8 {
        let mut bits = 0;
        bits.set_bit(0, self.display_bg[0]);
        bits.set_bit(1, self.display_bg[1]);
        bits.set_bit(2, self.display_bg[2]);
        bits.set_bit(3, self.display_bg[3]);
        bits.set_bit(4, self.display_obj);

        bits.set_bit(5, self.display_bg_window[0]);
        bits.set_bit(6, self.display_bg_window[1]);
        bits.set_bit(7, self.display_obj_window);

        bits
    }

    pub fn set_lo_bits(&mut self, bits: u8) {
        self.mode = bits.bits(..3);
        self.frame_select = bits.bits(4..5);
        self.hblank_oam_access = bits.bit(5);
        self.obj_1d = bits.bit(6);
        self.forced_blank = bits.bit(7);
    }

    pub fn set_hi_bits(&mut self, bits: u8) {
        self.display_bg[0] = bits.bit(0);
        self.display_bg[1] = bits.bit(1);
        self.display_bg[2] = bits.bit(2);
        self.display_bg[3] = bits.bit(3);
        self.display_obj = bits.bit(4);

        self.display_bg_window[0] = bits.bit(5);
        self.display_bg_window[1] = bits.bit(6);
        self.display_obj_window = bits.bit(7);
    }

    pub fn mode(&self) -> u8 {
        self.mode
    }

    pub fn mode_type(&self) -> Mode {
        match self.mode {
            0..=2 => Mode::Tile,
            3..=5 => Mode::Bitmap,
            _ => Mode::Invalid,
        }
    }

    pub fn frame_vram_offset(&self) -> usize {
        usize::from(self.frame_select) * 0xa000
    }

    pub fn obj_vram_offset(&self) -> usize {
        if self.mode_type() == Mode::Tile {
            0x1_0000
        } else {
            0x1_4000 // TODO: invalid type behaviour?
        }
    }

    pub(super) fn is_bg_hidden(&self, bg_idx: usize) -> bool {
        !self.display_bg[bg_idx]
            || (self.mode == 1 && bg_idx == 3)
            || (self.mode == 2 && bg_idx < 2)
            || (self.mode_type() == Mode::Bitmap && bg_idx != 2)
    }

    pub(super) fn bg_uses_text_mode(&self, bg_idx: usize) -> bool {
        self.mode == 0 || bg_idx < 2
    }
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Default, Copy, Clone, Debug)]
pub struct DisplayStatus {
    pub vblank_irq_enabled: bool,
    pub hblank_irq_enabled: bool,
    pub vcount_irq_enabled: bool,
    pub vcount_target: u8,
}

impl DisplayStatus {
    #[allow(clippy::similar_names)]
    pub fn lo_bits(self, vblanking: bool, hblanking: bool, vcount: u8) -> u8 {
        let mut bits = 0;
        bits.set_bit(0, vblanking);
        bits.set_bit(1, hblanking);
        bits.set_bit(2, vcount == self.vcount_target);
        bits.set_bit(3, self.vblank_irq_enabled);
        bits.set_bit(4, self.hblank_irq_enabled);
        bits.set_bit(5, self.vcount_irq_enabled);

        bits
    }

    #[allow(clippy::similar_names)]
    pub fn set_lo_bits(&mut self, bits: u8) {
        self.vblank_irq_enabled = bits.bit(3);
        self.hblank_irq_enabled = bits.bit(4);
        self.vcount_irq_enabled = bits.bit(5);
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub struct BackgroundControl {
    priority: u8,
    dots_base_block: u8,
    pub mosaic: bool,
    pub color256: bool,
    screen_base_block: u8,
    pub wraparound: bool,
    screen_size: u8,
    lo_bits: u8,
}

impl BackgroundControl {
    pub fn lo_bits(self) -> u8 {
        let mut bits = self.lo_bits;
        bits.set_bits(..2, self.priority);
        bits.set_bits(2..4, self.dots_base_block);
        bits.set_bit(6, self.mosaic);
        bits.set_bit(7, self.color256);

        bits
    }

    pub fn hi_bits(self) -> u8 {
        let mut bits = 0;
        bits.set_bits(..5, self.screen_base_block);
        bits.set_bit(5, self.wraparound);
        bits.set_bits(6.., self.screen_size);

        bits
    }

    pub fn set_lo_bits(&mut self, bits: u8) {
        self.priority = bits.bits(..2);
        self.dots_base_block = bits.bits(2..4);
        self.mosaic = bits.bit(6);
        self.color256 = bits.bit(7);
        self.lo_bits = bits;
    }

    pub fn set_hi_bits(&mut self, bits: u8) {
        self.screen_base_block = bits.bits(..5);
        self.wraparound = bits.bit(5);
        self.screen_size = bits.bits(6..);
    }

    pub fn priority(self) -> u8 {
        self.priority
    }

    pub(super) fn dots_vram_offset(self) -> usize {
        0x4000 * usize::from(self.dots_base_block)
    }

    pub fn screen_vram_offset(self, screen_idx: u8) -> usize {
        0x800 * usize::from(self.screen_base_block + screen_idx)
    }

    pub(super) fn screen_tile_len(self, text_mode: bool) -> u8 {
        if text_mode {
            32
        } else {
            16 << self.screen_size
        }
    }

    pub(super) fn text_mode_screen_index(self, screen_pos: (u32, u32)) -> u8 {
        let layout = match self.screen_size {
            0 => [[0, 0], [0, 0]],
            1 => [[0, 1], [0, 1]],
            2 => [[0, 0], [1, 1]],
            3 => [[0, 1], [2, 3]],
            _ => unreachable!(),
        };

        let (layout_x, layout_y) = ((screen_pos.0 % 2) as usize, (screen_pos.1 % 2) as usize);
        layout[layout_y % 2][layout_x % 2]
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub struct BackgroundOffset(u16, u16);

impl BackgroundOffset {
    pub fn get(self) -> (u16, u16) {
        (self.0, self.1)
    }

    pub fn set_x_lo_bits(&mut self, bits: u8) {
        self.0.set_bits(..8, bits.into());
    }

    pub fn set_x_hi_bits(&mut self, bits: u8) {
        self.0.set_bit(8, bits.bit(0));
    }

    pub fn set_y_lo_bits(&mut self, bits: u8) {
        self.1.set_bits(..8, bits.into());
    }

    pub fn set_y_hi_bits(&mut self, bits: u8) {
        self.1.set_bit(8, bits.bit(0));
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub struct ReferencePoint {
    external: (i32, i32),
    pub(super) internal: (i32, i32),
}

impl ReferencePoint {
    pub(super) fn external(self) -> (i32, i32) {
        self.external
    }

    fn set_byte(coord: &mut i32, idx: usize, bits: u8) {
        let bit_idx = idx * 8;
        match idx {
            0..=2 => coord.set_bits(bit_idx..bit_idx + 8, bits.into()),
            3 => {
                coord.set_bits(bit_idx..bit_idx + 4, bits.bits(..4).into());
                *coord = arbitrary_sign_extend!(i32, *coord, 28);
            }
            _ => panic!("byte index out of bounds"),
        }
    }

    pub fn set_x_byte(&mut self, idx: usize, bits: u8) {
        Self::set_byte(&mut self.external.0, idx, bits);
        self.internal.0 = self.external.0;
    }

    pub fn set_y_byte(&mut self, idx: usize, bits: u8) {
        Self::set_byte(&mut self.external.1, idx, bits);
        self.internal.1 = self.external.1;
    }
}

#[derive(Copy, Clone, Debug)]
pub struct BackgroundAffine {
    pub a: i16,
    pub b: i16,
    pub c: i16,
    pub d: i16,
}

impl Default for BackgroundAffine {
    fn default() -> Self {
        Self {
            a: 1 << 8,
            b: 0,
            c: 0,
            d: 1 << 8,
        }
    }
}

#[derive(Default, Copy, Clone, Debug)]
pub struct WindowDimensions {
    horiz: (u8, u8),
    vert: (u8, u8),
}

impl WindowDimensions {
    pub(super) fn horiz(self) -> (u8, u8) {
        self.horiz
    }

    pub(super) fn vert(self) -> (u8, u8) {
        self.vert
    }

    pub fn set_horiz_lo_bits(&mut self, bits: u8) {
        self.horiz.1 = bits;
    }

    pub fn set_horiz_hi_bits(&mut self, bits: u8) {
        self.horiz.0 = bits;
    }

    pub fn set_vert_lo_bits(&mut self, bits: u8) {
        self.vert.1 = bits;
    }

    pub fn set_vert_hi_bits(&mut self, bits: u8) {
        self.vert.0 = bits;
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub struct WindowControl {
    pub display_bg: [bool; 4],
    pub show_obj: bool,
    pub blendfx_enabled: bool,
    bits: u8,
}

impl WindowControl {
    pub fn bits(self) -> u8 {
        let mut bits = self.bits;
        bits.set_bit(0, self.display_bg[0]);
        bits.set_bit(1, self.display_bg[1]);
        bits.set_bit(2, self.display_bg[2]);
        bits.set_bit(3, self.display_bg[3]);
        bits.set_bit(4, self.show_obj);
        bits.set_bit(5, self.blendfx_enabled);

        bits
    }

    pub fn set_bits(&mut self, bits: u8) {
        self.display_bg[0] = bits.bit(0);
        self.display_bg[1] = bits.bit(1);
        self.display_bg[2] = bits.bit(2);
        self.display_bg[3] = bits.bit(3);
        self.show_obj = bits.bit(4);
        self.blendfx_enabled = bits.bit(5);
        self.bits = bits;
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub struct Mosaic(u8, u8);

impl Mosaic {
    pub fn get(self) -> (u8, u8) {
        (self.0, self.1)
    }

    pub fn set_bits(&mut self, bits: u8) {
        self.0 = bits.bits(..4);
        self.1 = bits.bits(4..);
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub struct BlendControl {
    pub bg_target: ([bool; 4], [bool; 4]),
    pub obj_target: (bool, bool),
    pub backdrop_target: (bool, bool),
    mode: u8,
    hi_bits: u8,
}

impl BlendControl {
    pub fn lo_bits(self) -> u8 {
        let mut bits = 0;
        bits.set_bit(0, self.bg_target.0[0]);
        bits.set_bit(1, self.bg_target.0[1]);
        bits.set_bit(2, self.bg_target.0[2]);
        bits.set_bit(3, self.bg_target.0[3]);
        bits.set_bit(4, self.obj_target.0);
        bits.set_bit(5, self.backdrop_target.0);
        bits.set_bits(6.., self.mode);

        bits
    }

    pub fn hi_bits(self) -> u8 {
        let mut bits = self.hi_bits;
        bits.set_bit(0, self.bg_target.1[0]);
        bits.set_bit(1, self.bg_target.1[1]);
        bits.set_bit(2, self.bg_target.1[2]);
        bits.set_bit(3, self.bg_target.1[3]);
        bits.set_bit(4, self.obj_target.1);
        bits.set_bit(5, self.backdrop_target.1);

        bits
    }

    pub fn set_lo_bits(&mut self, bits: u8) {
        self.bg_target.0[0] = bits.bit(0);
        self.bg_target.0[1] = bits.bit(1);
        self.bg_target.0[2] = bits.bit(2);
        self.bg_target.0[3] = bits.bit(3);
        self.obj_target.0 = bits.bit(4);
        self.backdrop_target.0 = bits.bit(5);
        self.mode = bits.bits(6..);
    }

    pub fn set_hi_bits(&mut self, bits: u8) {
        self.bg_target.1[0] = bits.bit(0);
        self.bg_target.1[1] = bits.bit(1);
        self.bg_target.1[2] = bits.bit(2);
        self.bg_target.1[3] = bits.bit(3);
        self.obj_target.1 = bits.bit(4);
        self.backdrop_target.1 = bits.bit(5);
        self.hi_bits = bits;
    }

    pub fn mode(self) -> u8 {
        self.mode
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub struct BlendCoefficient(pub u8);

impl BlendCoefficient {
    pub fn factor(self) -> f32 {
        1.0f32.min(f32::from(self.0.bits(..5)) / 16.0)
    }
}
