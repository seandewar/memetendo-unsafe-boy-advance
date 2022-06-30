mod reg;
pub mod screen;

use intbits::Bits;

use crate::{
    arm7tdmi::{Cpu, Exception},
    bus::Bus,
    video::reg::ModeType,
};

use self::{
    reg::{BackgroundControl, DisplayControl, DisplayStatus},
    screen::{FrameBuffer, Rgb, Screen},
};

const HORIZ_DOTS: u16 = 308;
const VERT_DOTS: u8 = 228;

const HBLANK_DOT: u16 = 240;
const VBLANK_DOT: u8 = 160;

const CYCLES_PER_DOT: u8 = 4;

pub struct Controller {
    x: u16,
    y: u8,
    cycle_accum: u8,
    frame_buf: FrameBuffer,

    pub palette_ram: Box<[u8]>,
    pub vram: Box<[u8]>,
    pub oam: Box<[u8]>,

    pub dispcnt: DisplayControl,
    pub dispstat: DisplayStatus,
    pub green_swap: u16,
    pub bgcnt: [BackgroundControl; 4],
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
            x: 0,
            y: 0,
            cycle_accum: 0,
            frame_buf: FrameBuffer::new(),
            palette_ram: vec![0; 0x400].into_boxed_slice(),
            vram: vec![0; 0x18000].into_boxed_slice(),
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
                let rgb = if self.dispcnt.forced_blank {
                    0xff_ff_ff.into()
                } else {
                    match self.dispcnt.mode_type() {
                        ModeType::Tile => self.compute_tile_mode_pixel(),
                        ModeType::Bitmap => self.compute_bitmap_mode_pixel(),
                        ModeType::Invalid => 0xff_ff_ff.into(), // TODO: what it do?
                    }
                };

                self.frame_buf.set_pixel(self.x.into(), self.y.into(), rgb);
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

    fn compute_tile_mode_pixel(&self) -> Rgb {
        const TILE_DIM: usize = 8;

        // TODO: other BGs, other sizes than 256, proper flipping, etc.
        let tile_x = usize::from(self.x) / TILE_DIM;
        let tile_y = usize::from(self.y) / TILE_DIM;
        let tile_idx = tile_y * (256 / TILE_DIM) + tile_x;
        let tile_info_offset = self.bgcnt[0].vram_offset() + 2 * tile_idx;

        #[allow(clippy::cast_possible_truncation)]
        let tile_info = self.vram.as_ref().read_hword(tile_info_offset as u32);
        let dots_idx = usize::from(tile_info.bits(..10));
        let flip_horiz = tile_info.bit(10);
        let flip_vert = tile_info.bit(11);

        let mut dot_x = usize::from(self.x) % TILE_DIM;
        if flip_horiz {
            dot_x = TILE_DIM - dot_x;
        }
        let mut dot_y = usize::from(self.y) % TILE_DIM;
        if flip_vert {
            dot_y = TILE_DIM - dot_y;
        }

        if self.bgcnt[0].color256 {
            0x00_ff_00.into() // TODO
        } else {
            // 4-bit depth
            let palette_group_idx = usize::from(tile_info.bits(12..));
            let dots_base_offset = self.bgcnt[0].dots_vram_offset() + 32 * dots_idx;
            let dots = self.vram[dots_base_offset + (4 * dot_y) + (dot_x / 2)];
            let palette_idx = usize::from(dots >> (4 * (dot_x % 2))).bits(..4);
            #[allow(clippy::cast_possible_truncation)]
            let palette_offset = 2 * (16 * palette_group_idx + palette_idx) as u32;

            Rgb::from_555(self.palette_ram.as_ref().read_hword(palette_offset))
        }
    }

    fn compute_bitmap_mode_pixel(&self) -> Rgb {
        if !self.dispcnt.display_bg[2] {
            return 0xff_ff_ff.into();
        }

        let (dot_x, dot_y) = (usize::from(self.x), usize::from(self.y));
        match self.dispcnt.mode {
            3 => {
                let dot_idx = dot_y * screen::WIDTH + dot_x;

                #[allow(clippy::cast_possible_truncation)]
                Rgb::from_555(self.vram.as_ref().read_hword(2 * dot_idx as u32))
            }
            4 => {
                let dot_idx = self.dispcnt.frame_vram_offset() + dot_y * screen::WIDTH + dot_x;
                // TODO: colour 0 is the backdrop colour, and also acts as transparent when
                //       rendering the object layer
                let palette_offset = u32::from(2 * self.vram[dot_idx]);

                Rgb::from_555(self.palette_ram.as_ref().read_hword(palette_offset))
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
