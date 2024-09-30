use intbits::Bits;
use strum_macros::FromRepr;
use tinyvec::array_vec;

use crate::{arbitrary_sign_extend, bus::Bus};

use super::{Video, HBLANK_DOT, VBLANK_DOT};

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum BackgroundMode {
    Tile,
    Bitmap,
    Invalid,
}

#[expect(clippy::struct_excessive_bools)]
#[derive(Default, Copy, Clone, Debug)]
pub(super) struct DisplayControl {
    pub mode: u8,
    frame_select: u8,
    hblank_oam_access: bool,
    pub obj_1d: bool,
    pub forced_blank: bool,
    pub display_bg: [bool; 4],
    pub display_obj: bool,
    pub display_bg_window: [bool; 2],
    pub display_obj_window: bool,
    cached_bits: u16,
}

impl DisplayControl {
    pub fn set_lo_bits(&mut self, bits: u8) {
        self.cached_bits.set_bits(..8, bits.into());
        self.mode = bits.bits(..3);
        self.frame_select = bits.bits(4..5);
        self.hblank_oam_access = bits.bit(5);
        self.obj_1d = bits.bit(6);
        self.forced_blank = bits.bit(7);
    }

    pub fn set_hi_bits(&mut self, bits: u8) {
        self.cached_bits.set_bits(8.., bits.into());
        self.display_bg[0] = bits.bit(0);
        self.display_bg[1] = bits.bit(1);
        self.display_bg[2] = bits.bit(2);
        self.display_bg[3] = bits.bit(3);
        self.display_obj = bits.bit(4);

        self.display_bg_window[0] = bits.bit(5);
        self.display_bg_window[1] = bits.bit(6);
        self.display_obj_window = bits.bit(7);
    }

    pub fn mode(&self) -> BackgroundMode {
        match self.mode {
            0..=2 => BackgroundMode::Tile,
            3..=5 => BackgroundMode::Bitmap,
            _ => BackgroundMode::Invalid,
        }
    }

    pub fn frame_vram_offset(&self) -> usize {
        usize::from(self.frame_select) * 0xa000
    }

    pub fn obj_vram_offset(&self) -> usize {
        match self.mode() {
            BackgroundMode::Tile => 0x1_0000,
            BackgroundMode::Bitmap | BackgroundMode::Invalid => 0x1_4000, // TODO: what does invalid actually do?
        }
    }
}

#[derive(Default, Copy, Clone, Debug)]
pub(super) struct DisplayStatus {
    pub vblank_irq_enabled: bool,
    pub hblank_irq_enabled: bool,
    pub vcount_irq_enabled: bool,
    pub vcount_target: u8,
    cached_bits: u8,
}

impl DisplayStatus {
    fn lo_bits(self, vblanking: bool, hblanking: bool, vcount: u8) -> u8 {
        self.cached_bits
            .with_bit(0, vblanking)
            .with_bit(1, hblanking)
            .with_bit(2, vcount == self.vcount_target)
    }

    fn set_lo_bits(&mut self, bits: u8) {
        self.cached_bits = bits;
        self.vblank_irq_enabled = bits.bit(3);
        self.hblank_irq_enabled = bits.bit(4);
        self.vcount_irq_enabled = bits.bit(5);
    }
}

#[derive(Copy, Clone, Default, Debug, FromRepr)]
#[repr(u8)]
pub(super) enum ScreenAreas {
    #[default]
    One,
    TwoHorizontal,
    TwoVertical,
    Four,
}

#[derive(Copy, Clone, Default, Debug)]
pub(super) struct BackgroundControl {
    pub priority: u8,
    dots_base_block: u8,
    pub mosaic: bool,
    pub color256: bool,
    screen_base_block: u8,
    pub wraparound: bool,
    screen_config: ScreenAreas,
    cached_bits: u16,
}

impl Video {
    fn set_bgcnt_lo_bits(&mut self, bg_idx: usize, bits: u8) {
        let old_priority = self.bgcnt[bg_idx].priority;
        self.bgcnt[bg_idx].set_lo_bits(bits);

        if old_priority != self.bgcnt[bg_idx].priority {
            self.priority_sort_tile_mode_bgs();
        }
    }
}

impl BackgroundControl {
    pub fn set_lo_bits(&mut self, bits: u8) {
        self.cached_bits.set_bits(..8, bits.into());
        self.priority = bits.bits(..2);
        self.dots_base_block = bits.bits(2..4);
        self.mosaic = bits.bit(6);
        self.color256 = bits.bit(7);
    }

    pub fn set_hi_bits(&mut self, bits: u8) {
        self.cached_bits.set_bits(8.., bits.into());
        self.screen_base_block = bits.bits(..5);
        self.wraparound = bits.bit(5);
        self.screen_config = ScreenAreas::from_repr(bits.bits(6..)).unwrap();
    }

    pub fn dots_vram_offset(self) -> usize {
        0x4000 * usize::from(self.dots_base_block)
    }

    pub fn screen_vram_offset(self, screen_idx: u8) -> usize {
        0x800 * usize::from(self.screen_base_block + screen_idx)
    }

    pub fn screen_tile_len(self, text_mode: bool) -> u8 {
        if text_mode {
            32
        } else {
            16 << self.screen_config as u8
        }
    }

    pub fn text_mode_screen_index(self, (screen_x, screen_y): (i32, i32)) -> u8 {
        let layout = match self.screen_config {
            ScreenAreas::One => [[0, 0], [0, 0]],
            ScreenAreas::TwoHorizontal => [[0, 1], [0, 1]],
            ScreenAreas::TwoVertical => [[0, 0], [1, 1]],
            ScreenAreas::Four => [[0, 1], [2, 3]],
        };

        layout[usize::try_from(screen_y.rem_euclid(2)).unwrap()]
            [usize::try_from(screen_x.rem_euclid(2)).unwrap()]
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub(super) struct BackgroundOffset(u16, u16);

impl BackgroundOffset {
    pub fn get(self) -> (u16, u16) {
        (self.0, self.1)
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub(super) struct ReferencePoint {
    pub external: (i32, i32),
    pub internal: (i32, i32),
}

impl ReferencePoint {
    fn set_byte(coord: &mut i32, idx: usize, bits: u8) {
        let bit_idx = idx * 8;
        match idx {
            0..=2 => coord.set_bits(bit_idx..bit_idx + 8, bits.into()),
            3 => {
                coord.set_bits(bit_idx..bit_idx + 4, bits.bits(..4).into());
                *coord = arbitrary_sign_extend!(i32, *coord, 28);
            }
            _ => unreachable!(),
        }
    }

    fn set_x_byte(&mut self, idx: usize, bits: u8) {
        Self::set_byte(&mut self.external.0, idx, bits);
        self.internal.0 = self.external.0;
    }

    fn set_y_byte(&mut self, idx: usize, bits: u8) {
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
    pub horiz: (u8, u8),
    pub vert: (u8, u8),
}

#[derive(Copy, Clone, Default, Debug)]
pub(super) struct WindowControl {
    pub display_bg: [bool; 4],
    pub display_obj: bool,
    pub blendfx_enabled: bool,
    cached_bits: u8,
}

impl WindowControl {
    fn set_bits(&mut self, bits: u8) {
        self.cached_bits = bits;
        self.display_bg[0] = bits.bit(0);
        self.display_bg[1] = bits.bit(1);
        self.display_bg[2] = bits.bit(2);
        self.display_bg[3] = bits.bit(3);
        self.display_obj = bits.bit(4);
        self.blendfx_enabled = bits.bit(5);
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub(super) struct MosaicSize(u8, u8);

impl MosaicSize {
    fn set_bits(&mut self, bits: u8) {
        self.0 = bits.bits(..4) + 1;
        self.1 = bits.bits(4..) + 1;
    }

    pub fn get(self) -> (u8, u8) {
        (self.0, self.1)
    }
}

#[derive(Copy, Clone, Eq, PartialEq, FromRepr, Default, Debug)]
#[repr(u8)]
pub(super) enum BlendMode {
    #[default]
    None,
    Alpha,
    Brighten,
    Dim,
}

#[derive(Copy, Clone, Default, Debug)]
pub(super) struct BlendControl {
    pub bg_target: [[bool; 4]; 2],
    pub obj_target: [bool; 2],
    pub backdrop_target: [bool; 2],
    pub mode: BlendMode,
    cached_bits: u16,
}

impl BlendControl {
    fn set_lo_bits(&mut self, bits: u8) {
        self.cached_bits.set_bits(..8, bits.into());
        self.bg_target[0][0] = bits.bit(0);
        self.bg_target[0][1] = bits.bit(1);
        self.bg_target[0][2] = bits.bit(2);
        self.bg_target[0][3] = bits.bit(3);
        self.obj_target[0] = bits.bit(4);
        self.backdrop_target[0] = bits.bit(5);
        self.mode = BlendMode::from_repr(bits.bits(6..)).unwrap();
    }

    fn set_hi_bits(&mut self, bits: u8) {
        self.cached_bits.set_bits(8.., bits.into());
        self.bg_target[1][0] = bits.bit(0);
        self.bg_target[1][1] = bits.bit(1);
        self.bg_target[1][2] = bits.bit(2);
        self.bg_target[1][3] = bits.bit(3);
        self.obj_target[1] = bits.bit(4);
        self.backdrop_target[1] = bits.bit(5);
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub(super) struct BlendCoefficient(u8);

impl BlendCoefficient {
    pub fn factor(self) -> f32 {
        1.0f32.min(f32::from(self.0.bits(..5)) / 16.0)
    }
}

impl Bus for Video {
    fn read_byte(&mut self, addr: u32) -> u8 {
        match addr {
            // DISPCNT
            0x00 => self.dispcnt.cached_bits.bits(..8).try_into().unwrap(),
            0x01 => self.dispcnt.cached_bits.bits(8..).try_into().unwrap(),
            // GREENSWP (undocumented)
            0x02 => self.greenswp.bits(..8).try_into().unwrap(),
            0x03 => self.greenswp.bits(8..).try_into().unwrap(),
            // DISPSTAT
            0x04 => self.dispstat.lo_bits(
                self.y >= VBLANK_DOT && self.y != 227,
                self.x >= HBLANK_DOT.into(),
                self.y,
            ),
            0x05 => self.dispstat.vcount_target,
            // VCOUNT
            0x06 => self.y,
            // BG0CNT
            0x08 => self.bgcnt[0].cached_bits.bits(..8).try_into().unwrap(),
            0x09 => self.bgcnt[0].cached_bits.bits(8..).try_into().unwrap(),
            // BG1CNT
            0x0a => self.bgcnt[1].cached_bits.bits(..8).try_into().unwrap(),
            0x0b => self.bgcnt[1].cached_bits.bits(8..).try_into().unwrap(),
            // BG2CNT
            0x0c => self.bgcnt[2].cached_bits.bits(..8).try_into().unwrap(),
            0x0d => self.bgcnt[2].cached_bits.bits(8..).try_into().unwrap(),
            // BG3CNT
            0x0e => self.bgcnt[3].cached_bits.bits(..8).try_into().unwrap(),
            0x0f => self.bgcnt[3].cached_bits.bits(8..).try_into().unwrap(),
            // WININ
            0x48 => self.winin[0].cached_bits,
            0x49 => self.winin[1].cached_bits,
            // WINOUT
            0x4a => self.winout.cached_bits,
            0x4b => self.winobj.cached_bits,
            // BLDCNT
            0x50 => self.bldcnt.cached_bits.bits(..8).try_into().unwrap(),
            0x51 => self.bldcnt.cached_bits.bits(8..).try_into().unwrap(),
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
            0x00 => {
                let old_mode = self.dispcnt.mode;
                self.dispcnt.set_lo_bits(value);

                if old_mode != self.dispcnt.mode && self.dispcnt.mode() == BackgroundMode::Tile {
                    self.tile_mode_bg_order = match self.dispcnt.mode {
                        0 => array_vec![0, 1, 2, 3],
                        1 => array_vec![0, 1, 2],
                        2 => array_vec![2, 3],
                        _ => unreachable!(),
                    };
                    self.priority_sort_tile_mode_bgs();
                }
            }
            0x01 => self.dispcnt.set_hi_bits(value),
            // GREENSWP (undocumented)
            0x02 => self.greenswp.set_bits(..8, value.into()),
            0x03 => self.greenswp.set_bits(8.., value.into()),
            // DISPSTAT
            0x04 => self.dispstat.set_lo_bits(value),
            0x05 => self.dispstat.vcount_target = value,
            // BG0CNT
            0x08 => self.set_bgcnt_lo_bits(0, value),
            0x09 => self.bgcnt[0].set_hi_bits(value),
            // BG1CNT
            0x0a => self.set_bgcnt_lo_bits(1, value),
            0x0b => self.bgcnt[1].set_hi_bits(value),
            // BG2CNT
            0x0c => self.set_bgcnt_lo_bits(2, value),
            0x0d => self.bgcnt[2].set_hi_bits(value),
            // BG3CNT
            0x0e => self.set_bgcnt_lo_bits(3, value),
            0x0f => self.bgcnt[3].set_hi_bits(value),
            // BG0HOFS
            0x10 => self.bgofs[0].0.set_bits(..8, value.into()),
            0x11 => self.bgofs[0].0.set_bit(8, value.bit(0)),
            // BG0VOFS
            0x12 => self.bgofs[0].1.set_bits(..8, value.into()),
            0x13 => self.bgofs[0].1.set_bit(8, value.bit(0)),
            // BG1HOFS
            0x14 => self.bgofs[1].0.set_bits(..8, value.into()),
            0x15 => self.bgofs[1].0.set_bit(8, value.bit(0)),
            // BG1VOFS
            0x16 => self.bgofs[1].1.set_bits(..8, value.into()),
            0x17 => self.bgofs[1].1.set_bit(8, value.bit(0)),
            // BG2HOFS
            0x18 => self.bgofs[2].0.set_bits(..8, value.into()),
            0x19 => self.bgofs[2].0.set_bit(8, value.bit(0)),
            // BG2VOFS
            0x1a => self.bgofs[2].1.set_bits(..8, value.into()),
            0x1b => self.bgofs[2].1.set_bit(8, value.bit(0)),
            // BG3HOFS
            0x1c => self.bgofs[3].0.set_bits(..8, value.into()),
            0x1d => self.bgofs[3].0.set_bit(8, value.bit(0)),
            // BG3VOFS
            0x1e => self.bgofs[3].1.set_bits(..8, value.into()),
            0x1f => self.bgofs[3].1.set_bit(8, value.bit(0)),
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
            0x28..=0x2b => self.bgref[0].set_x_byte((addr & 3).try_into().unwrap(), value),
            // BG2Y
            0x2c..=0x2f => self.bgref[0].set_y_byte((addr & 3).try_into().unwrap(), value),
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
            0x38..=0x3b => self.bgref[1].set_x_byte((addr & 3).try_into().unwrap(), value),
            // BG3Y
            0x3c..=0x3f => self.bgref[1].set_y_byte((addr & 3).try_into().unwrap(), value),
            // WIN0H
            0x40 => self.win[0].horiz.1 = value,
            0x41 => self.win[0].horiz.0 = value,
            // WIN1H
            0x42 => self.win[1].horiz.1 = value,
            0x43 => self.win[1].horiz.0 = value,
            // WIN0V
            0x44 => self.win[0].vert.1 = value,
            0x45 => self.win[0].vert.0 = value,
            // WIN1V
            0x46 => self.win[1].vert.1 = value,
            0x47 => self.win[1].vert.0 = value,
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
