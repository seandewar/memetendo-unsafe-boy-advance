use intbits::Bits;

use crate::{arbitrary_sign_extend, bus::Bus};

use super::{Video, HBLANK_DOT, VBLANK_DOT};

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Mode {
    Tile,
    Bitmap,
    Invalid,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Default, Copy, Clone, Debug)]
pub(super) struct DisplayControl {
    mode: u8,
    frame_select: u8,
    hblank_oam_access: bool,
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
        match self.mode_type() {
            Mode::Tile => 0x1_0000,
            Mode::Bitmap | Mode::Invalid => 0x1_4000, // TODO: what does invalid actually do?
        }
    }

    pub(super) fn bg_uses_text_mode(&self, bg_idx: usize) -> bool {
        self.mode == 0 || bg_idx < 2
    }
}

#[derive(Default, Copy, Clone, Debug)]
pub(super) struct DisplayStatus {
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
pub(super) struct BackgroundControl {
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

    pub(super) fn text_mode_screen_index(self, (screen_x, screen_y): (u32, u32)) -> u8 {
        let layout = match self.screen_size {
            0 => [[0, 0], [0, 0]],
            1 => [[0, 1], [0, 1]],
            2 => [[0, 0], [1, 1]],
            3 => [[0, 1], [2, 3]],
            _ => unreachable!(),
        };

        let (layout_x, layout_y) = ((screen_x % 2) as usize, (screen_y % 2) as usize);
        layout[layout_y % 2][layout_x % 2]
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub(super) struct BackgroundOffset(u16, u16);

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
pub(super) struct ReferencePoint {
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
pub(super) struct BackgroundAffine {
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
pub(super) struct WindowDimensions {
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
pub(super) struct WindowControl {
    pub display_bg: [bool; 4],
    pub display_obj: bool,
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
        bits.set_bit(4, self.display_obj);
        bits.set_bit(5, self.blendfx_enabled);

        bits
    }

    pub fn set_bits(&mut self, bits: u8) {
        self.display_bg[0] = bits.bit(0);
        self.display_bg[1] = bits.bit(1);
        self.display_bg[2] = bits.bit(2);
        self.display_bg[3] = bits.bit(3);
        self.display_obj = bits.bit(4);
        self.blendfx_enabled = bits.bit(5);
        self.bits = bits;
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub(super) struct Mosaic(u8, u8);

impl Mosaic {
    pub fn set_bits(&mut self, bits: u8) {
        self.0 = bits.bits(..4);
        self.1 = bits.bits(4..);
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub(super) struct BlendControl {
    pub bg_target: [[bool; 4]; 2],
    pub obj_target: [bool; 2],
    pub backdrop_target: [bool; 2],
    mode: u8,
    hi_bits: u8,
}

impl BlendControl {
    pub fn lo_bits(self) -> u8 {
        let mut bits = 0;
        bits.set_bit(0, self.bg_target[0][0]);
        bits.set_bit(1, self.bg_target[0][1]);
        bits.set_bit(2, self.bg_target[0][2]);
        bits.set_bit(3, self.bg_target[0][3]);
        bits.set_bit(4, self.obj_target[0]);
        bits.set_bit(5, self.backdrop_target[0]);
        bits.set_bits(6.., self.mode);

        bits
    }

    pub fn hi_bits(self) -> u8 {
        let mut bits = self.hi_bits;
        bits.set_bit(0, self.bg_target[1][0]);
        bits.set_bit(1, self.bg_target[1][1]);
        bits.set_bit(2, self.bg_target[1][2]);
        bits.set_bit(3, self.bg_target[1][3]);
        bits.set_bit(4, self.obj_target[1]);
        bits.set_bit(5, self.backdrop_target[1]);

        bits
    }

    pub fn set_lo_bits(&mut self, bits: u8) {
        self.bg_target[0][0] = bits.bit(0);
        self.bg_target[0][1] = bits.bit(1);
        self.bg_target[0][2] = bits.bit(2);
        self.bg_target[0][3] = bits.bit(3);
        self.obj_target[0] = bits.bit(4);
        self.backdrop_target[0] = bits.bit(5);
        self.mode = bits.bits(6..);
    }

    pub fn set_hi_bits(&mut self, bits: u8) {
        self.bg_target[1][0] = bits.bit(0);
        self.bg_target[1][1] = bits.bit(1);
        self.bg_target[1][2] = bits.bit(2);
        self.bg_target[1][3] = bits.bit(3);
        self.obj_target[1] = bits.bit(4);
        self.backdrop_target[1] = bits.bit(5);
        self.hi_bits = bits;
    }

    pub fn mode(self) -> u8 {
        self.mode
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub(super) struct BlendCoefficient(pub u8);

impl BlendCoefficient {
    pub fn factor(self) -> f32 {
        1.0f32.min(f32::from(self.0.bits(..5)) / 16.0)
    }
}

impl Bus for Video {
    fn read_byte(&mut self, addr: u32) -> u8 {
        #[allow(clippy::cast_possible_truncation)]
        match addr {
            // DISPCNT
            0x0 => self.dispcnt.lo_bits(),
            0x1 => self.dispcnt.hi_bits(),
            // GREENSWP (undocumented)
            0x2 => self.greenswp as u8,
            0x3 => self.greenswp.bits(8..) as u8,
            // DISPSTAT
            0x4 => self.dispstat.lo_bits(
                self.y >= VBLANK_DOT && self.y != 227,
                self.x >= HBLANK_DOT.into(),
                self.y,
            ),
            0x5 => self.dispstat.vcount_target,
            // VCOUNT
            0x6 => self.y,
            // BG0CNT
            0x8 => self.bgcnt[0].lo_bits(),
            0x9 => self.bgcnt[0].hi_bits(),
            // BG1CNT
            0xa => self.bgcnt[1].lo_bits(),
            0xb => self.bgcnt[1].hi_bits(),
            // BG2CNT
            0xc => self.bgcnt[2].lo_bits(),
            0xd => self.bgcnt[2].hi_bits(),
            // BG3CNT
            0xe => self.bgcnt[3].lo_bits(),
            0xf => self.bgcnt[3].hi_bits(),
            // WININ
            0x48 => self.winin[0].bits(),
            0x49 => self.winin[1].bits(),
            // WINOUT
            0x4a => self.winout.bits(),
            0x4b => self.winobj.bits(),
            // BLDCNT
            0x50 => self.bldcnt.lo_bits(),
            0x51 => self.bldcnt.hi_bits(),
            // BLDALPHA
            0x52 => self.bldalpha.0 .0,
            0x53 => self.bldalpha.1 .0,
            0x57.. => panic!("IO register address OOB"),
            _ => 0,
        }
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        match addr {
            // DISPCNT
            0x0 => self.set_dispcnt_lo_bits(value),
            0x1 => self.set_dispcnt_hi_bits(value),
            // GREENSWP (undocumented)
            0x2 => self.greenswp.set_bits(..8, value.into()),
            0x3 => self.greenswp.set_bits(8.., value.into()),
            // DISPSTAT
            0x4 => self.dispstat.set_lo_bits(value),
            0x5 => self.dispstat.vcount_target = value,
            // BG0CNT
            0x8 => self.set_bgcnt_lo_bits(0, value),
            0x9 => self.set_bgcnt_hi_bits(0, value),
            // BG1CNT
            0xa => self.set_bgcnt_lo_bits(1, value),
            0xb => self.set_bgcnt_hi_bits(1, value),
            // BG2CNT
            0xc => self.set_bgcnt_lo_bits(2, value),
            0xd => self.set_bgcnt_hi_bits(2, value),
            // BG3CNT
            0xe => self.set_bgcnt_lo_bits(3, value),
            0xf => self.set_bgcnt_hi_bits(3, value),
            // BG0HOFS
            0x10 => self.bgofs[0].set_x_lo_bits(value),
            0x11 => self.bgofs[0].set_x_hi_bits(value),
            // BG0VOFS
            0x12 => self.bgofs[0].set_y_lo_bits(value),
            0x13 => self.bgofs[0].set_y_hi_bits(value),
            // BG1HOFS
            0x14 => self.bgofs[1].set_x_lo_bits(value),
            0x15 => self.bgofs[1].set_x_hi_bits(value),
            // BG1VOFS
            0x16 => self.bgofs[1].set_y_lo_bits(value),
            0x17 => self.bgofs[1].set_y_hi_bits(value),
            // BG2HOFS
            0x18 => self.bgofs[2].set_x_lo_bits(value),
            0x19 => self.bgofs[2].set_x_hi_bits(value),
            // BG2VOFS
            0x1a => self.bgofs[2].set_y_lo_bits(value),
            0x1b => self.bgofs[2].set_y_hi_bits(value),
            // BG3HOFS
            0x1c => self.bgofs[3].set_x_lo_bits(value),
            0x1d => self.bgofs[3].set_x_hi_bits(value),
            // BG3VOFS
            0x1e => self.bgofs[3].set_y_lo_bits(value),
            0x1f => self.bgofs[3].set_y_hi_bits(value),
            // BG2PA
            0x20 => self.bgp[0].a.set_bits(..8, value.into()),
            0x21 => self.bgp[0].a.set_bits(8.., value.into()),
            // BG2PB
            0x22 => self.bgp[0].b.set_bits(..8, value.into()),
            0x23 => self.bgp[0].b.set_bits(8.., value.into()),
            // BG2PC
            0x24 => self.bgp[0].c.set_bits(..8, value.into()),
            0x25 => self.bgp[0].c.set_bits(8.., value.into()),
            // BG2PD
            0x26 => self.bgp[0].d.set_bits(..8, value.into()),
            0x27 => self.bgp[0].d.set_bits(8.., value.into()),
            // BG2X
            offset @ 0x28..=0x2b => self.bgref[0].set_x_byte((offset & 3) as usize, value),
            // BG2Y
            offset @ 0x2c..=0x2f => self.bgref[0].set_y_byte((offset & 3) as usize, value),
            // BG3PA
            0x30 => self.bgp[1].a.set_bits(..8, value.into()),
            0x31 => self.bgp[1].a.set_bits(8.., value.into()),
            // BG3PB
            0x32 => self.bgp[1].b.set_bits(..8, value.into()),
            0x33 => self.bgp[1].b.set_bits(8.., value.into()),
            // BG3PC
            0x34 => self.bgp[1].c.set_bits(..8, value.into()),
            0x35 => self.bgp[1].c.set_bits(8.., value.into()),
            // BG3PD
            0x36 => self.bgp[1].d.set_bits(..8, value.into()),
            0x37 => self.bgp[1].d.set_bits(8.., value.into()),
            // BG3X
            offset @ 0x38..=0x3b => self.bgref[1].set_x_byte((offset & 3) as usize, value),
            // BG3Y
            offset @ 0x3c..=0x3f => self.bgref[1].set_y_byte((offset & 3) as usize, value),
            // WIN0H
            0x40 => self.win[0].set_horiz_lo_bits(value),
            0x41 => self.win[0].set_horiz_hi_bits(value),
            // WIN1H
            0x42 => self.win[1].set_horiz_lo_bits(value),
            0x43 => self.win[1].set_horiz_hi_bits(value),
            // WIN0V
            0x44 => self.win[0].set_vert_lo_bits(value),
            0x45 => self.win[0].set_vert_hi_bits(value),
            // WIN1V
            0x46 => self.win[1].set_vert_lo_bits(value),
            0x47 => self.win[1].set_vert_hi_bits(value),
            // WININ
            0x48 => self.winin[0].set_bits(value),
            0x49 => self.winin[1].set_bits(value),
            // WINOUT
            0x4a => self.winout.set_bits(value),
            0x4b => self.winobj.set_bits(value),
            // MOSAIC
            0x4c => self.mosaic_bg.set_bits(value),
            0x4d => self.mosaic_obj.set_bits(value),
            // BLDCNT
            0x50 => self.bldcnt.set_lo_bits(value),
            0x51 => self.bldcnt.set_hi_bits(value),
            // BLDALPHA
            0x52 => self.bldalpha.0 .0 = value,
            0x53 => self.bldalpha.1 .0 = value,
            // BLDY
            0x54 => self.bldy.0 = value,
            0x57.. => panic!("IO register address OOB"),
            _ => {}
        }
    }
}
