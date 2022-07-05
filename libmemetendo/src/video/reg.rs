use intbits::Bits;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Mode {
    Tile,
    Bitmap,
    Invalid,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Default, Debug)]
pub struct DisplayControl {
    pub mode: u8,
    pub frame_select: usize,
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
        #[allow(clippy::cast_possible_truncation)]
        bits.set_bits(4..5, self.frame_select as u8);
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
        self.frame_select = bits.bits(4..5).into();
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

    pub fn frame_vram_offset(&self) -> usize {
        self.frame_select * 0xa000
    }

    pub fn mode_type(&self) -> Mode {
        match self.mode {
            0..=2 => Mode::Tile,
            3..=5 => Mode::Bitmap,
            _ => Mode::Invalid,
        }
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
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Default, Debug)]
pub struct DisplayStatus {
    pub vblank_irq_enabled: bool,
    pub hblank_irq_enabled: bool,
    pub vcount_irq_enabled: bool,
    pub unused_bit7: bool,
    pub vcount_target: u8,
}

impl DisplayStatus {
    #[allow(clippy::similar_names)]
    pub fn lo_bits(&self, vblanking: bool, hblanking: bool, vcount: u8) -> u8 {
        let mut bits = 0;
        bits.set_bit(0, vblanking);
        bits.set_bit(1, hblanking);
        bits.set_bit(2, vcount == self.vcount_target);
        bits.set_bit(3, self.vblank_irq_enabled);
        bits.set_bit(4, self.hblank_irq_enabled);
        bits.set_bit(5, self.vcount_irq_enabled);
        bits.set_bit(7, self.unused_bit7);

        bits
    }

    #[allow(clippy::similar_names)]
    pub fn set_lo_bits(&mut self, bits: u8) {
        self.vblank_irq_enabled = bits.bit(3);
        self.hblank_irq_enabled = bits.bit(4);
        self.vcount_irq_enabled = bits.bit(5);
        self.unused_bit7 = bits.bit(7);
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub struct BackgroundControl {
    pub priority: u8,
    pub dots_base_block: u8,
    pub unused_bit4_5: u8,
    pub mosaic: bool,
    pub color256: bool,
    pub screen_base_block: u8,
    pub wraparound: bool,
    pub screen_size: u8,
}

impl BackgroundControl {
    pub fn lo_bits(self) -> u8 {
        let mut bits = 0;
        bits.set_bits(..2, self.priority);
        bits.set_bits(2..4, self.dots_base_block);
        bits.set_bits(4..6, self.unused_bit4_5);
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
        self.unused_bit4_5 = bits.bits(4..6);
        self.mosaic = bits.bit(6);
        self.color256 = bits.bit(7);
    }

    pub fn set_hi_bits(&mut self, bits: u8) {
        self.screen_base_block = bits.bits(..5);
        self.wraparound = bits.bit(5);
        self.screen_size = bits.bits(6..);
    }

    pub(super) fn dots_vram_offset(
        self,
        color256: bool,
        dots_idx: usize,
        dot_x: usize,
        dot_y: usize,
    ) -> usize {
        let size_div = if color256 { 1 } else { 2 };
        let bytes_per_tile = 64 / size_div;
        let base_offset = 0x4000 * usize::from(self.dots_base_block) + bytes_per_tile * dots_idx;

        base_offset + (8 * dot_y + dot_x) / size_div
    }

    pub fn screen_vram_offset(self, screen_idx: u8) -> usize {
        0x800 * usize::from(self.screen_base_block + screen_idx)
    }

    pub fn screen_index(self, screen_x: usize, screen_y: usize) -> u8 {
        let layout = match self.screen_size {
            0 => [0, 0, 0, 0],
            1 => [0, 1, 0, 1],
            2 => [0, 0, 1, 1],
            3 => [0, 1, 2, 3],
            _ => unreachable!(),
        };

        layout[(screen_y % 2) * 2 + (screen_x % 2)]
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub struct BackgroundOffset(u16);

impl BackgroundOffset {
    pub fn get(self) -> u16 {
        self.0
    }

    pub fn set_lo_bits(&mut self, bits: u8) {
        self.0.set_bits(..8, bits.into());
    }

    pub fn set_hi_bits(&mut self, bits: u8) {
        self.0.set_bit(8, bits.bit(0));
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub struct WindowControl {
    pub display_bg: [bool; 4],
    pub show_obj: bool,
    pub blendfx_enabled: bool,
    pub unused_bit6_7: u8,
}

impl WindowControl {
    pub fn bits(self) -> u8 {
        let mut bits = 0;
        bits.set_bit(0, self.display_bg[0]);
        bits.set_bit(1, self.display_bg[1]);
        bits.set_bit(2, self.display_bg[2]);
        bits.set_bit(3, self.display_bg[3]);
        bits.set_bit(4, self.show_obj);
        bits.set_bit(5, self.blendfx_enabled);
        bits.set_bits(6.., self.unused_bit6_7);

        bits
    }

    pub fn set_bits(&mut self, bits: u8) {
        self.display_bg[0] = bits.bit(0);
        self.display_bg[1] = bits.bit(1);
        self.display_bg[2] = bits.bit(2);
        self.display_bg[3] = bits.bit(3);
        self.show_obj = bits.bit(4);
        self.blendfx_enabled = bits.bit(5);
        self.unused_bit6_7 = bits.bits(6..);
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
    pub mode: u8,
    pub unused_bit14_15: u8,
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
        let mut bits = 0;
        bits.set_bit(0, self.bg_target.1[0]);
        bits.set_bit(1, self.bg_target.1[1]);
        bits.set_bit(2, self.bg_target.1[2]);
        bits.set_bit(3, self.bg_target.1[3]);
        bits.set_bit(4, self.obj_target.1);
        bits.set_bit(5, self.backdrop_target.1);
        bits.set_bits(6.., self.unused_bit14_15);

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
        self.unused_bit14_15 = bits.bits(6..);
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub struct BlendAlpha(u8, u8);

impl BlendAlpha {
    pub fn get(self) -> (u8, u8) {
        (self.0, self.1)
    }

    pub fn set_lo_bits(&mut self, bits: u8) {
        self.0 = bits.bits(..5);
    }

    pub fn set_hi_bits(&mut self, bits: u8) {
        self.1 = bits.bits(..5);
    }

    pub fn blend_factor(self) -> (f32, f32) {
        (
            1.0f32.min(f32::from(self.0) / 16.0),
            1.0f32.min(f32::from(self.1) / 16.0),
        )
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub struct BlendBrightness(u8);

impl BlendBrightness {
    pub fn get(self) -> u8 {
        self.0
    }

    pub fn set_bits(&mut self, bits: u8) {
        self.0 = bits.bits(..4);
    }
}
