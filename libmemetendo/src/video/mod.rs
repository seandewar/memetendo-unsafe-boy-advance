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
    pub bgofs: [(u8, u8); 4],
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
            bgofs: [(0, 0); 4],
        }
    }

    pub fn step(&mut self, screen: &mut impl Screen, cpu: &mut Cpu, cycles: u32) {
        for _ in 0..cycles {
            if self.x < HBLANK_DOT && self.y < VBLANK_DOT {
                let rgb = if self.dispcnt.forced_blank {
                    0xff_ff_ff.into()
                } else {
                    let rgb = match self.dispcnt.mode_type() {
                        ModeType::Tile => self.compute_tile_mode_pixel(),
                        ModeType::Bitmap => self.compute_bitmap_mode_pixel(),
                        ModeType::Invalid => None, // TODO: what it do?
                    };

                    // If transparent, use the backdrop colour.
                    rgb.unwrap_or_else(|| Rgb::from_555(self.palette_ram.as_ref().read_hword(0)))
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

    fn compute_tile_mode_pixel(&self) -> Option<Rgb> {
        const TILE_DIM: usize = 8;

        let tile_x = usize::from(self.x) / TILE_DIM;
        let tile_y = usize::from(self.y) / TILE_DIM;
        let tile_idx = tile_y * (256 / TILE_DIM) + tile_x;

        // BGs with the same priority set are prioritized based on index (smallest idx = highest).
        let mut bg_priorities = [3, 2, 1, 0];
        bg_priorities.sort_unstable_by(|&a, &b| {
            self.bgcnt[b]
                .priority
                .cmp(&self.bgcnt[a].priority)
                .then_with(|| b.cmp(&a))
        });

        let mut rgb = None;
        for bg_idx in bg_priorities {
            if !self.dispcnt.display_bg[bg_idx]
                || (self.dispcnt.mode == 1 && bg_idx == 3)
                || (self.dispcnt.mode == 2 && bg_idx < 2)
            {
                continue;
            }

            let tile_info_offset = self.bgcnt[bg_idx].vram_offset() + 2 * tile_idx;
            #[allow(clippy::cast_possible_truncation)]
            let tile_info = self.vram.as_ref().read_hword(tile_info_offset as u32);
            let dots_idx = usize::from(tile_info.bits(..10));
            let flip_horiz = tile_info.bit(10);
            let flip_vert = tile_info.bit(11);

            let mut dot_x = usize::from(self.x) % TILE_DIM;
            if flip_horiz {
                dot_x = TILE_DIM - 1 - dot_x;
            }
            let mut dot_y = usize::from(self.y) % TILE_DIM;
            if flip_vert {
                dot_y = TILE_DIM - 1 - dot_y;
            }

            let color256 = self.bgcnt[bg_idx].color256
                || (self.dispcnt.mode == 1 && bg_idx == 2)
                || self.dispcnt.mode == 2;
            let palette_color_idx = if color256 {
                // 8-bit depth
                let dots_base_offset = self.bgcnt[bg_idx].dots_vram_offset() + 64 * dots_idx;

                u32::from(self.vram[dots_base_offset + 8 * dot_y + dot_x])
            } else {
                // 4-bit depth
                let dots_base_offset = self.bgcnt[bg_idx].dots_vram_offset() + 32 * dots_idx;
                let dots = self.vram[dots_base_offset + 4 * dot_y + (dot_x / 2)];
                #[allow(clippy::cast_possible_truncation)]
                let palette_offset_idx = u32::from(dots >> (4 * (dot_x as u8 % 2))).bits(..4);

                palette_offset_idx
            };

            if palette_color_idx != 0 {
                let color_idx = if color256 {
                    palette_color_idx
                } else {
                    16 * u32::from(tile_info.bits(12..)) + palette_color_idx
                };

                rgb = Some(Rgb::from_555(
                    self.palette_ram.as_ref().read_hword(2 * color_idx),
                ));
            }
        }

        rgb
    }

    fn compute_bitmap_mode_pixel(&self) -> Option<Rgb> {
        if !self.dispcnt.display_bg[2] {
            return None;
        }

        let (dot_x, dot_y) = (usize::from(self.x), usize::from(self.y));
        match self.dispcnt.mode {
            3 => {
                let dot_idx = dot_y * screen::WIDTH + dot_x;
                #[allow(clippy::cast_possible_truncation)]
                let rgb = Rgb::from_555(self.vram.as_ref().read_hword(2 * dot_idx as u32));

                Some(rgb)
            }
            4 => {
                let dot_idx = self.dispcnt.frame_vram_offset() + dot_y * screen::WIDTH + dot_x;
                let color_idx = u32::from(self.vram[dot_idx]);
                let rgb = Rgb::from_555(self.palette_ram.as_ref().read_hword(2 * color_idx));

                Some(rgb)
            }
            5 if dot_x >= 160 || dot_y >= 128 => None,
            5 => {
                let dot_idx = self.dispcnt.frame_vram_offset() + dot_y * 160 + dot_x;
                #[allow(clippy::cast_possible_truncation)]
                let rgb = Rgb::from_555(self.vram.as_ref().read_hword(2 * dot_idx as u32));

                Some(rgb)
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
