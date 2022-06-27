mod reg;

use std::ops::{Index, IndexMut};

use intbits::Bits;

use crate::{
    arm7tdmi::{Cpu, Exception},
    bus::Bus,
    video::reg::ModeType,
};

use self::reg::{BackgroundControl, DisplayControl, DisplayStatus};

pub const FRAME_WIDTH: usize = HBLANK_DOT as _;
pub const FRAME_HEIGHT: usize = VBLANK_DOT as _;

#[derive(Clone, Debug)]
pub struct FrameBuffer(pub Box<[u32]>);

impl Default for FrameBuffer {
    fn default() -> Self {
        Self(vec![0; FRAME_WIDTH * FRAME_HEIGHT].into_boxed_slice())
    }
}

impl Index<(usize, usize)> for FrameBuffer {
    type Output = u32;

    fn index(&self, index: (usize, usize)) -> &Self::Output {
        &self.0[index.1 * FRAME_WIDTH + index.0]
    }
}

impl IndexMut<(usize, usize)> for FrameBuffer {
    fn index_mut(&mut self, index: (usize, usize)) -> &mut Self::Output {
        &mut self.0[index.1 * FRAME_WIDTH + index.0]
    }
}

pub trait Screen {
    fn present_frame(&mut self, frame_buf: &FrameBuffer);
}

const HORIZ_DOTS: u16 = 308;
const VERT_DOTS: u8 = 228;

const HBLANK_DOT: u16 = 240;
const VBLANK_DOT: u8 = 160;

const CYCLES_PER_DOT: u8 = 4;

pub struct Controller {
    frame_buf: FrameBuffer,
    cycle_accum: u8,
    x: u16,
    y: u8,

    pub palette_ram: Box<[u8]>,
    pub vram: Box<[u8]>,
    pub oam: Box<[u8]>,

    pub dispcnt: DisplayControl,
    pub dispstat: DisplayStatus,
    pub green_swap: u16,
    pub bgcnt: [BackgroundControl; 4],
}

#[allow(clippy::cast_possible_truncation)]
fn rgb555_to_24(value: u16) -> u32 {
    let r = value.bits(..5) as u8;
    let g = value.bits(5..10) as u8;
    let b = value.bits(10..15) as u8;

    u32::from_le_bytes([r * 8, g * 8, b * 8, 0])
}

impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}

impl Controller {
    #[must_use]
    pub fn new() -> Self {
        Self {
            frame_buf: FrameBuffer::default(),
            cycle_accum: 0,
            x: 0,
            y: 0,
            palette_ram: vec![0; 0x400].into_boxed_slice(),
            vram: vec![0; 0x1_8000].into_boxed_slice(),
            oam: vec![0; 0x400].into_boxed_slice(),
            dispcnt: DisplayControl::default(),
            dispstat: DisplayStatus::default(),
            green_swap: 0,
            bgcnt: [BackgroundControl::default(); 4],
        }
    }

    pub fn step(&mut self, screen: &mut impl Screen, cpu: &mut Cpu, cycles: u32) {
        for _ in 0..cycles {
            if self.x < HBLANK_DOT && self.y < VBLANK_DOT {
                self.frame_buf[(self.x.into(), self.y.into())] = self.compute_colour();
            }

            self.cycle_accum += 1;
            if self.cycle_accum >= CYCLES_PER_DOT {
                self.cycle_accum = 0;
                self.x += 1;

                if self.x == HBLANK_DOT && self.y == VBLANK_DOT - 1 {
                    screen.present_frame(&self.frame_buf);
                }

                let mut irq =
                    self.dispstat.hblank_irq_enabled && self.x == HBLANK_DOT && self.y < VBLANK_DOT;

                if self.x >= HORIZ_DOTS {
                    self.x = 0;
                    self.y += 1;

                    if self.y >= VERT_DOTS {
                        self.y = 0;
                    }

                    irq |= self.dispstat.vblank_irq_enabled && self.y == VBLANK_DOT;
                    irq |=
                        self.dispstat.vcount_irq_enabled && self.y == self.dispstat.vcount_target;
                }

                if irq {
                    cpu.raise_exception(Exception::Interrupt);
                }
            }
        }
    }

    fn compute_colour(&self) -> u32 {
        const TILE_DIMENSION: usize = 8;

        if self.dispcnt.forced_blank {
            return 0xff_ff_ff;
        }

        // TODO: palette colour 0 is always transparent
        match self.dispcnt.mode_type() {
            ModeType::Tile => {
                // TODO: other BGs, other sizes than 256, proper flipping, etc.
                let tile_x = usize::from(self.x) / TILE_DIMENSION;
                let tile_y = usize::from(self.y) / TILE_DIMENSION;
                let tile_idx = tile_y * (256 / TILE_DIMENSION) + tile_x;
                let tile_info_offset = self.bgcnt[0].vram_offset() + 2 * tile_idx;

                #[allow(clippy::cast_possible_truncation)]
                let tile_info = self.vram.as_ref().read_hword(tile_info_offset as u32);
                let dots_idx = usize::from(tile_info.bits(..10));
                let flip_horiz = tile_info.bit(10);
                let flip_vert = tile_info.bit(11);

                let mut dot_x = usize::from(self.x) % TILE_DIMENSION;
                if flip_horiz {
                    dot_x = TILE_DIMENSION - dot_x;
                }
                let mut dot_y = usize::from(self.y) % TILE_DIMENSION;
                if flip_vert {
                    dot_y = TILE_DIMENSION - dot_y;
                }

                if self.bgcnt[0].color256 {
                    0x00_ff_00 // TODO
                } else {
                    // 4-bit depth
                    let palette_group_idx = usize::from(tile_info.bits(12..));
                    let dots_base_offset = self.bgcnt[0].dots_vram_offset() + 32 * dots_idx;
                    let dots = self.vram[dots_base_offset + (4 * dot_y) + (dot_x / 2)];
                    let palette_idx = usize::from(dots >> (4 * (dot_x % 2))).bits(..4);
                    #[allow(clippy::cast_possible_truncation)]
                    let palette_offset = 2 * (16 * palette_group_idx + palette_idx) as u32;

                    rgb555_to_24(self.palette_ram.as_ref().read_hword(palette_offset))
                }
            }
            ModeType::Bitmap if self.dispcnt.display_bg[2] => {
                let (dot_x, dot_y) = (usize::from(self.x), usize::from(self.y));

                match self.dispcnt.mode {
                    3 => {
                        let dot_idx = dot_y * FRAME_WIDTH + dot_x;

                        #[allow(clippy::cast_possible_truncation)]
                        rgb555_to_24(self.vram.as_ref().read_hword(2 * dot_idx as u32))
                    }
                    4 => {
                        let dot_idx =
                            self.dispcnt.frame_vram_offset() + dot_y * FRAME_WIDTH + dot_x;
                        // TODO: colour 0 is the backdrop colour, and also acts as transparent when
                        //       rendering the object layer
                        let palette_offset = u32::from(2 * self.vram[dot_idx]);

                        rgb555_to_24(self.palette_ram.as_ref().read_hword(palette_offset))
                    }
                    5 => {
                        // TODO: this is actually 160x128 pixels, we probably want to rescale...
                        let dot_idx = self.dispcnt.frame_vram_offset() + dot_y * 160 + dot_x;

                        let _ = dot_idx;
                        todo!();
                    }
                    _ => unreachable!(),
                }
            }
            _ => 0xff_ff_ff,
        }
    }

    #[must_use]
    pub fn dispstat_lo_bits(&self) -> u8 {
        self.dispstat.lo_bits(
            self.y >= VBLANK_DOT && self.y != 227,
            self.x >= HBLANK_DOT,
            self.y,
        )
    }

    #[must_use]
    pub fn vcount(&self) -> u8 {
        self.y
    }
}
