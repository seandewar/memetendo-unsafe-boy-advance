use std::ops::{Index, IndexMut};

use intbits::Bits;

pub const FRAME_WIDTH: usize = 240;
pub const FRAME_HEIGHT: usize = 160;

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
const VERT_DOTS: u16 = 228;
const CYCLES_PER_DOT: u8 = 4;

pub(super) struct VideoController {
    frame_buf: FrameBuffer,
    dot_cycle_accum: u8,
    dot_x: u16,
    dot_y: u16,

    pub(super) palette_ram: Box<[u8]>,
    pub(super) vram: Box<[u8]>,
    pub(super) oam: Box<[u8]>,
    pub(super) dispcnt: u32,
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
            dot_cycle_accum: 0,
            dot_x: 0,
            dot_y: 0,
            palette_ram: vec![0; 0x400].into_boxed_slice(),
            vram: vec![0; 0x1_8000].into_boxed_slice(),
            oam: vec![0; 0x400].into_boxed_slice(),
            dispcnt: 0,
        }
    }

    pub fn step(&mut self, screen: &mut impl Screen, cycles: u32) {
        for _ in 0..cycles {
            if !self.is_hblanking() && !self.is_vblanking() {
                let (x, y) = (self.dot_x.into(), self.dot_y.into());

                let rgb = if self.dispcnt.bit(7) {
                    // Forced blank; TODO: memory not accessed
                    0xff_ff_ff
                } else {
                    // TODO
                    let i = y * FRAME_WIDTH + x;
                    u32::from(self.vram[i]) * 0xff
                };
                self.frame_buf[(x, y)] = rgb;
            }

            self.dot_cycle_accum += 1;
            if self.dot_cycle_accum >= CYCLES_PER_DOT {
                self.dot_cycle_accum = 0;
                self.dot_x += 1;

                if usize::from(self.dot_x) == FRAME_WIDTH
                    && usize::from(self.dot_y) == FRAME_HEIGHT - 1
                {
                    screen.present_frame(&self.frame_buf);
                }

                if self.dot_x >= HORIZ_DOTS {
                    self.dot_x = 0;
                    self.dot_y += 1;

                    if self.dot_y >= VERT_DOTS {
                        self.dot_y = 0;
                    }
                }
            }
        }
    }

    fn is_hblanking(&self) -> bool {
        usize::from(self.dot_x) >= FRAME_WIDTH
    }

    fn is_vblanking(&self) -> bool {
        usize::from(self.dot_y) >= FRAME_HEIGHT
    }
}
