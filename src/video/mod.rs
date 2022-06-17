mod reg;

use std::ops::{Index, IndexMut};

use intbits::Bits;

use crate::arm7tdmi::{Cpu, Exception};

use self::reg::{DisplayControl, DisplayStatus};

pub const FRAME_WIDTH: usize = HBLANK_DOT as _;
pub const FRAME_HEIGHT: usize = VBLANK_DOT as _;

#[derive(Debug)]
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

pub(super) struct VideoController {
    frame_buf: FrameBuffer,
    cycle_accum: u8,
    x: u16,
    y: u8,

    pub(super) palette_ram: Box<[u8]>,
    pub(super) vram: Box<[u8]>,
    pub(super) oam: Box<[u8]>,

    pub(super) dispcnt: DisplayControl,
    pub(super) dispstat: DisplayStatus,
    pub(super) green_swap: u16,
}

impl Default for VideoController {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(clippy::cast_possible_truncation)]
fn bgr555_to_24(value: u16) -> u32 {
    let r = value.bits(..5) as u8;
    let g = value.bits(5..10) as u8;
    let b = value.bits(10..15) as u8;

    u32::from_le_bytes([r << 3, g << 3, b << 3, 0])
}

impl VideoController {
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
        }
    }

    #[allow(clippy::similar_names)]
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
        if self.dispcnt.forced_blank {
            return 0xff_ff_ff;
        }

        let (x, y) = (usize::from(self.x), usize::from(self.y));
        if (3..=5).contains(&self.dispcnt.mode) && self.dispcnt.display_bg[2] {
            match self.dispcnt.mode {
                3 => {
                    let dot_idx = y * FRAME_WIDTH + x;
                    let lo = self.vram[2 * dot_idx];
                    let hi = self.vram[2 * dot_idx + 1];

                    bgr555_to_24(u16::from_le_bytes([lo, hi]))
                }
                4 => {
                    let dot_idx = y * FRAME_WIDTH + x + self.dispcnt.frame_vram_index();
                    let palette_idx = 2 * usize::from(self.vram[dot_idx]); // TODO: 0==transparent
                    let lo = self.palette_ram[palette_idx];
                    let hi = self.palette_ram[palette_idx + 1];

                    bgr555_to_24(u16::from_le_bytes([lo, hi]))
                }
                5 => {
                    // TODO: this is actually 160x128 pixels, we probably want to rescale...
                    let dot_idx = y * 160 + x + self.dispcnt.frame_vram_index();

                    let _ = dot_idx;
                    todo!();
                }
                _ => unreachable!(),
            }
        } else {
            0 // TODO
        }
    }

    pub(super) fn dispstat_lo_bits(&self) -> u8 {
        self.dispstat.lo_bits(
            self.y >= VBLANK_DOT && self.y != 227,
            self.x >= HBLANK_DOT,
            self.y,
        )
    }

    pub(super) fn vcount(&self) -> u8 {
        self.y
    }
}
