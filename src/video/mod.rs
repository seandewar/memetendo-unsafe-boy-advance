mod reg;

use std::ops::{Index, IndexMut};

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
                self.frame_buf[(self.x.into(), self.y.into())] = self.compute_rgb();
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

    fn compute_rgb(&self) -> u32 {
        if self.dispcnt.forced_blank {
            return 0xff_ff_ff;
        }

        let (x, y) = (usize::from(self.x), usize::from(self.y));
        let i = y * FRAME_WIDTH + x + usize::from(self.dispcnt.frame_select) * 0xa000;
        if self.vram[i] > 0 {
            0xff_ff_ff
        } else {
            0
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
