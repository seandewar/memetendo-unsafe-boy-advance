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
    pub bgofs: [(u16, u16); 4],
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
                        ModeType::Tile => self.compute_tile_pixel(),
                        ModeType::Bitmap => self.compute_bitmap_pixel(),
                        ModeType::Invalid => None, // TODO: what it do?
                    };

                    // If transparent, use the backdrop colour.
                    rgb.unwrap_or_else(|| Rgb::from_555(self.palette_ram.as_ref().read_hword(0)))
                };

                self.frame_buf
                    .set_pixel(self.x.into(), self.y.into(), rgb, self.green_swap.bit(0));
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

    fn bg_draw_order_iter(&self) -> impl Iterator<Item = usize> + '_ {
        // If many BGs share the same priority, the one with the smallest index wins.
        let mut bg_priorities = [3, 2, 1, 0];
        bg_priorities.sort_unstable_by(|&a, &b| {
            self.bgcnt[b]
                .priority
                .cmp(&self.bgcnt[a].priority)
                .then_with(|| b.cmp(&a))
        });

        bg_priorities
            .into_iter()
            .filter(|&i| !self.dispcnt.is_bg_hidden(i))
    }

    fn compute_tile_pixel(&self) -> Option<Rgb> {
        const TILE_LEN: usize = 8;
        const SCREEN_TILE_LEN: usize = 32;

        let mut rgb = None;
        for bg_idx in self.bg_draw_order_iter() {
            let scroll_x = usize::from(self.bgofs[bg_idx].0.bits(..9));
            let scroll_y = usize::from(self.bgofs[bg_idx].1.bits(..9));
            let x = scroll_x + usize::from(self.x);
            let y = scroll_y + usize::from(self.y);
            let (tile_x, tile_y) = (x / TILE_LEN, y / TILE_LEN);

            let (screen_x, screen_y) = (tile_x / SCREEN_TILE_LEN, tile_y / SCREEN_TILE_LEN);
            let screen_idx = self.bgcnt[bg_idx].screen_index(screen_x, screen_y);
            let screen_base_offset = self.bgcnt[bg_idx].screen_vram_offset(screen_idx);
            let screen_tile_x = tile_x % SCREEN_TILE_LEN;
            let screen_tile_y = tile_y % SCREEN_TILE_LEN;
            let screen_tile_idx = screen_tile_y * SCREEN_TILE_LEN + screen_tile_x;

            #[allow(clippy::cast_possible_truncation)]
            let tile_info_offset = (screen_base_offset + 2 * screen_tile_idx) as u32;
            let tile_info = self.vram.as_ref().read_hword(tile_info_offset);
            let dots_idx = usize::from(tile_info.bits(..10));
            let flip_horiz = tile_info.bit(10);
            let flip_vert = tile_info.bit(11);

            let (mut dot_x, mut dot_y) = (x % TILE_LEN, y % TILE_LEN);
            if flip_horiz {
                dot_x = TILE_LEN - 1 - dot_x;
            }
            if flip_vert {
                dot_y = TILE_LEN - 1 - dot_y;
            }

            let color256 = self.bgcnt[bg_idx].color256
                || (self.dispcnt.mode == 1 && bg_idx == 2)
                || self.dispcnt.mode == 2;
            let dots_offset = self.bgcnt[bg_idx].dots_vram_offset(color256, dots_idx, dot_x, dot_y);

            #[allow(clippy::cast_possible_truncation)]
            let palette_color_idx = if color256 {
                u32::from(self.vram[dots_offset])
            } else {
                // 4-bit depth
                u32::from(self.vram[dots_offset] >> (4 * (dot_x as u8 % 2))).bits(..4)
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

    fn compute_bitmap_pixel(&self) -> Option<Rgb> {
        if !self.dispcnt.display_bg[2] {
            return None;
        }

        let color_ram = if self.dispcnt.mode == 4 {
            &self.palette_ram
        } else {
            &self.vram
        };
        let (dot_x, dot_y) = (usize::from(self.x), usize::from(self.y));
        let color_idx = match self.dispcnt.mode {
            3 => Some(dot_y * screen::WIDTH + dot_x),
            4 => {
                let dot_offset = self.dispcnt.frame_vram_offset() + dot_y * screen::WIDTH + dot_x;

                Some(self.vram[dot_offset].into())
            }
            5 if dot_x >= 160 || dot_y >= 128 => None,
            5 => Some(self.dispcnt.frame_vram_offset() + dot_y * 160 + dot_x),
            _ => unreachable!(),
        };

        #[allow(clippy::cast_possible_truncation)]
        color_idx
            .map(|i| 2 * i)
            .filter(|&offset| offset < color_ram.len())
            .map(|offset| Rgb::from_555(color_ram.as_ref().read_hword(offset as u32)))
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
