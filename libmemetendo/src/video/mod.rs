mod reg;
pub mod screen;

use intbits::Bits;

use crate::{
    arm7tdmi::{Cpu, Exception},
    bus::Bus,
    video::reg::Mode,
};

use self::{
    reg::{
        BackgroundControl, BackgroundOffset, BlendCoefficient, BlendControl, DisplayControl,
        DisplayStatus, Mosaic, RefPoint, WindowControl,
    },
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
    pub greenswp: u16,
    pub bgcnt: [BackgroundControl; 4],
    pub bgofs: [(BackgroundOffset, BackgroundOffset); 4],
    pub bg_ref: [RefPoint; 2],
    pub bgp: [[i16; 4]; 2],
    pub winh: [(u8, u8); 2],
    pub winv: [(u8, u8); 2],
    pub winin: [WindowControl; 2],
    pub winout: WindowControl,
    pub winobj: WindowControl,
    pub mosaic_bg: Mosaic,
    pub mosaic_obj: Mosaic,
    pub bldcnt: BlendControl,
    pub bldalpha: (BlendCoefficient, BlendCoefficient),
    pub bldy: BlendCoefficient,
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
            greenswp: 0,
            bgcnt: [BackgroundControl::default(); 4],
            bgofs: [(BackgroundOffset::default(), BackgroundOffset::default()); 4],
            bg_ref: [RefPoint::default(); 2],
            bgp: [[1 << 8, 0, 0, 1 << 8]; 2],
            winh: [(0, 0); 2],
            winv: [(0, 0); 2],
            winin: [WindowControl::default(); 2],
            winout: WindowControl::default(),
            winobj: WindowControl::default(),
            mosaic_bg: Mosaic::default(),
            mosaic_obj: Mosaic::default(),
            bldcnt: BlendControl::default(),
            bldalpha: (BlendCoefficient::default(), BlendCoefficient::default()),
            bldy: BlendCoefficient::default(),
        }
    }

    #[allow(clippy::similar_names)]
    pub fn step(&mut self, screen: &mut impl Screen, cpu: &mut Cpu, cycles: u32) {
        for _ in 0..cycles {
            if self.x < HBLANK_DOT && self.y < VBLANK_DOT {
                self.frame_buf.set_pixel(
                    self.x.into(),
                    self.y.into(),
                    self.compute_rgb(),
                    self.greenswp.bit(0),
                );
            }

            self.cycle_accum += 1;
            if self.cycle_accum >= CYCLES_PER_DOT {
                self.cycle_accum = 0;
                self.x += 1;
                if self.x == HBLANK_DOT && self.y == VBLANK_DOT - 1 {
                    screen.present_frame(&self.frame_buf);
                }

                let mut irq = false;
                if self.x == HBLANK_DOT && self.y < VBLANK_DOT {
                    irq |= self.dispstat.hblank_irq_enabled;

                    for (i, bg_ref) in self.bg_ref.iter_mut().enumerate() {
                        let (dmx, dmy) = (i32::from(self.bgp[i][1]), i32::from(self.bgp[i][3]));
                        bg_ref.internal.0 += dmx;
                        bg_ref.internal.1 += dmy;
                    }
                }

                if self.x >= HORIZ_DOTS {
                    self.x = 0;
                    self.y += 1;
                    if self.y == VBLANK_DOT {
                        irq |= self.dispstat.vblank_irq_enabled;

                        for bg_ref in &mut self.bg_ref {
                            bg_ref.internal = bg_ref.external();
                        }
                    } else if self.y >= VERT_DOTS {
                        self.y = 0;
                    }

                    irq |=
                        self.dispstat.vcount_irq_enabled && self.y == self.dispstat.vcount_target;
                }

                if irq {
                    cpu.raise_exception(Exception::Interrupt);
                }
            }
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

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct Dot {
    r: u8,
    g: u8,
    b: u8,
}

const WHITE_DOT: Dot = Dot {
    r: 31,
    g: 31,
    b: 31,
};

impl From<u16> for Dot {
    #[allow(clippy::cast_possible_truncation)]
    fn from(value: u16) -> Self {
        Dot {
            r: value.bits(..5) as u8,
            g: value.bits(5..10) as u8,
            b: value.bits(10..15) as u8,
        }
    }
}

impl Dot {
    fn to_rgb(self) -> Rgb {
        debug_assert!(self.r < 32 && self.g < 32 && self.b < 32);

        Rgb {
            r: self.r * 8,
            g: self.g * 8,
            b: self.b * 8,
        }
    }
}

impl Controller {
    fn compute_rgb(&self) -> Rgb {
        if self.dispcnt.forced_blank {
            WHITE_DOT.to_rgb()
        } else {
            let (x, y) = (usize::from(self.x), usize::from(self.y));
            let mosaic_offset = (
                x % usize::from(self.mosaic_bg.get().0 + 1),
                y % usize::from(self.mosaic_bg.get().1 + 1),
            );

            if mosaic_offset == (0, 0) {
                // TODO: object layer, rotation & scaling
                let dot = match self.dispcnt.mode_type() {
                    Mode::Tile => self.compute_tile_mode_dot(),
                    // TODO: windows, blending fx
                    Mode::Bitmap => self.compute_bg_bitmap_mode_dot(),
                    Mode::Invalid => self.read_backdrop_dot(), // TODO: what it do?
                };

                dot.to_rgb()
            } else {
                self.frame_buf
                    .pixel(x - mosaic_offset.0, y - mosaic_offset.1)
            }
        }
    }

    fn read_backdrop_dot(&self) -> Dot {
        Dot::from(self.palette_ram.as_ref().read_hword(0))
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum Visibility {
    Visible,
    InsideWindow(usize),
    OutsideWindows,
    Hidden,
}

impl Controller {
    fn bg_priority_iter(&self) -> impl Iterator<Item = usize> + '_ {
        // If many BGs share the same priority, the one with the smallest index wins.
        let mut bg_priorities = [0, 1, 2, 3];
        bg_priorities.sort_unstable_by(|&a, &b| {
            self.bgcnt[a]
                .priority
                .cmp(&self.bgcnt[b].priority)
                .then_with(|| a.cmp(&b))
        });

        bg_priorities.into_iter()
    }

    fn compute_dot_visibility(&self, bg_idx: Option<usize>) -> Visibility {
        if bg_idx.map_or(false, |i| self.dispcnt.is_bg_hidden(i)) {
            return Visibility::Hidden;
        }
        if self.dispcnt.display_bg_window == [false; 2] && !self.dispcnt.display_obj_window {
            return Visibility::Visible; // All windows are disabled; show everything.
        }

        let mut visibility = if bg_idx.map_or(false, |i| !self.winout.display_bg[i]) {
            Visibility::Hidden
        } else {
            Visibility::OutsideWindows
        };
        for win_idx in (0..2).rev() {
            let win_x = (self.winh[win_idx].1, self.winh[win_idx].0);
            let win_y = (self.winv[win_idx].1, self.winv[win_idx].0);

            let inside_horiz = if win_x.0 <= win_x.1 {
                self.x >= win_x.0.into() && self.x < win_x.1.into()
            } else {
                self.x < win_x.1.into() || self.x >= win_x.0.into()
            };
            let inside_vert = if win_y.0 <= win_y.1 {
                self.y >= win_y.0 && self.y < win_y.1
            } else {
                self.y < win_y.1 || self.y >= win_y.0
            };

            if inside_horiz && inside_vert {
                if self.dispcnt.display_bg_window[win_idx]
                    && bg_idx.map_or(true, |i| self.winin[win_idx].display_bg[i])
                {
                    visibility = Visibility::InsideWindow(win_idx);
                } else if bg_idx.is_some() {
                    visibility = Visibility::Hidden;
                }
            }
        }

        visibility
    }

    fn compute_tile_mode_dot(&self) -> Dot {
        let mut bg_iter = self.bg_priority_iter();
        let (bg_idx, mut dot, visibility) = bg_iter
            .by_ref()
            .map(|i| (i, self.compute_bg_tile_mode_dot(i)))
            .find(|(_, (dot, _))| dot.is_some())
            .map_or_else(
                || {
                    (
                        None,
                        self.read_backdrop_dot(),
                        self.compute_dot_visibility(None),
                    )
                },
                |(i, (dot, vis))| (Some(i), dot.unwrap(), vis),
            );

        let target_blendfx = bg_idx.map_or(self.bldcnt.backdrop_target.0, |i| {
            self.bldcnt.bg_target.0[i]
        });
        let win_blendfx = match visibility {
            Visibility::Visible => true,
            Visibility::InsideWindow(i) => self.winin[i].blendfx_enabled,
            Visibility::OutsideWindows => self.winout.blendfx_enabled,
            Visibility::Hidden => false,
        };
        let blendfx = self.bldcnt.mode != 0 && target_blendfx && win_blendfx;

        if blendfx {
            dot = match self.bldcnt.mode {
                1 => self.alpha_blend_dot(bg_iter, dot),
                2 => self.brighten_dot(false, dot),
                3 => self.brighten_dot(true, dot),
                _ => unreachable!(),
            };
        }

        dot
    }

    fn brighten_dot(&self, darken: bool, dot: Dot) -> Dot {
        let mul = if darken {
            |comp| -f32::from(comp)
        } else {
            |comp| f32::from(31 - comp)
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let blend = |comp| (f32::from(comp) + mul(comp) * self.bldy.factor()) as u8;

        Dot {
            r: blend(dot.r),
            g: blend(dot.g),
            b: blend(dot.b),
        }
    }

    fn alpha_blend_dot(&self, bg_iter: impl Iterator<Item = usize>, dot: Dot) -> Dot {
        let bot_dot = bg_iter
            .filter(|&i| self.bldcnt.bg_target.1[i])
            .map(|i| self.compute_bg_tile_mode_dot(i).0)
            .find(Option::is_some)
            .flatten()
            .or_else(|| {
                self.bldcnt
                    .backdrop_target
                    .1
                    .then(|| self.read_backdrop_dot())
            });

        if let Some(bot_dot) = bot_dot {
            let factor = (self.bldalpha.0.factor(), self.bldalpha.1.factor());
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let blend = |top: u8, bot: u8| {
                31.min((f32::from(top) * factor.0 + f32::from(bot) * factor.1) as u32) as u8
            };

            Dot {
                r: blend(dot.r, bot_dot.r),
                g: blend(dot.g, bot_dot.g),
                b: blend(dot.b, bot_dot.b),
            }
        } else {
            dot
        }
    }

    fn transformed_pos(&self, bg_idx: usize) -> (i32, i32) {
        let (x, y) = (i32::from(self.x), i32::from(self.y));

        if self.dispcnt.bg_uses_text_mode(bg_idx) {
            let scroll = &self.bgofs[bg_idx];
            let (scroll_x, scroll_y) = (i32::from(scroll.0.get()), i32::from(scroll.1.get()));

            (scroll_x + x, scroll_y + y)
        } else {
            let transform = &self.bgp[bg_idx - 2];
            let ref_point = &self.bg_ref[bg_idx - 2];
            let (dx, dy) = (i32::from(transform[0]), i32::from(transform[2]));
            let (ref_x, ref_y) = ref_point.internal;
            let (new_x, new_y) = (ref_x.wrapping_add(x * dx), ref_y.wrapping_add(x * dy));

            (new_x >> 8, new_y >> 8)
        }
    }

    fn compute_bg_tile_mode_dot(&self, bg_idx: usize) -> (Option<Dot>, Visibility) {
        const TILE_LEN: usize = 8;

        let visibility = self.compute_dot_visibility(Some(bg_idx));
        if visibility == Visibility::Hidden {
            return (None, visibility);
        }

        let (x, y) = self.transformed_pos(bg_idx);
        if x < 0 || y < 0 {
            return (None, visibility);
        }

        #[allow(clippy::cast_sign_loss)]
        let (x, y) = (x as usize, y as usize);
        let tile_pos = (x / TILE_LEN, y / TILE_LEN);

        let text_mode = self.dispcnt.bg_uses_text_mode(bg_idx);
        let screen_tile_len = if text_mode {
            32
        } else {
            16 << usize::from(self.bgcnt[bg_idx].screen_size)
        };
        let screen_pos = (tile_pos.0 / screen_tile_len, tile_pos.1 / screen_tile_len);
        let screen_idx = if text_mode {
            self.bgcnt[bg_idx].text_mode_screen_index(screen_pos.0, screen_pos.1)
        } else {
            0
        };
        let screen_base_offset = self.bgcnt[bg_idx].screen_vram_offset(screen_idx);
        let screen_wraparound = text_mode || self.bgcnt[bg_idx].wraparound;

        let screen_tile_pos = if screen_wraparound {
            (tile_pos.0 % screen_tile_len, tile_pos.1 % screen_tile_len)
        } else {
            tile_pos
        };
        if screen_tile_pos.0 >= screen_tile_len || screen_tile_pos.1 >= screen_tile_len {
            return (None, visibility);
        }

        let screen_tile_idx = screen_tile_pos.1 * screen_tile_len + screen_tile_pos.0;

        let (mut dot_x, mut dot_y) = (x % TILE_LEN, y % TILE_LEN);
        let (dots_idx, palette_idx) = if text_mode {
            #[allow(clippy::cast_possible_truncation)]
            let tile_info_offset = (screen_base_offset + 2 * screen_tile_idx) as u32;
            let tile_info = self.vram.as_ref().read_hword(tile_info_offset);
            let dots_idx = usize::from(tile_info.bits(..10));

            if self.dispcnt.mode == 0 || (self.dispcnt.mode == 1 && bg_idx < 2) {
                if tile_info.bit(10) {
                    dot_x = TILE_LEN - 1 - dot_x; // Flip X
                }
                if tile_info.bit(11) {
                    dot_y = TILE_LEN - 1 - dot_y; // Flip Y
                }
            }

            let color256 = self.bgcnt[bg_idx].color256
                || (self.dispcnt.mode == 1 && bg_idx == 2)
                || self.dispcnt.mode == 2;
            let palette_idx = (!color256).then(|| tile_info.bits(12..));

            (dots_idx, palette_idx)
        } else {
            let dots_idx_offset = screen_base_offset + screen_tile_idx;
            if dots_idx_offset >= self.vram.len() {
                return (None, visibility);
            }

            (self.vram[dots_idx_offset].into(), None)
        };

        let dots_offset =
            self.bgcnt[bg_idx].dots_vram_offset(palette_idx.is_none(), dots_idx, dot_x, dot_y);

        #[allow(clippy::cast_possible_truncation)]
        let palette_color_idx = if palette_idx.is_some() {
            u32::from(self.vram[dots_offset] >> (4 * (dot_x as u8 % 2))).bits(..4)
        } else {
            u32::from(self.vram[dots_offset])
        };

        let dot = (palette_color_idx != 0).then(|| {
            let color_idx = if let Some(palette_idx) = palette_idx {
                16 * u32::from(palette_idx) + palette_color_idx
            } else {
                palette_color_idx
            };

            Dot::from(self.palette_ram.as_ref().read_hword(2 * color_idx))
        });

        (dot, visibility)
    }

    // TODO: backdrop colour needs to be treated as transparent
    fn compute_bg_bitmap_mode_dot(&self) -> Dot {
        if self.compute_dot_visibility(Some(2)) == Visibility::Hidden {
            return self.read_backdrop_dot();
        }

        let color_ram = if self.dispcnt.mode == 4 {
            &self.palette_ram
        } else {
            &self.vram
        };

        let (x, y) = self.transformed_pos(2);
        #[allow(clippy::cast_sign_loss)]
        let (x, y) = (x as usize, y as usize);
        let color_offset = match self.dispcnt.mode {
            3 | 4 if x >= screen::WIDTH || y >= screen::HEIGHT => None,
            3 => Some(2 * (y * screen::WIDTH + x)),
            4 => Some(
                2 * usize::from(
                    self.vram[self.dispcnt.frame_vram_offset() + y * screen::WIDTH + x],
                ),
            ),
            5 if x >= 160 || y >= 128 => None,
            5 => Some(self.dispcnt.frame_vram_offset() + 2 * (y * 160 + x)),
            _ => unreachable!(),
        };

        #[allow(clippy::cast_possible_truncation)]
        color_offset
            .filter(|&offset| offset < color_ram.len())
            .map_or_else(
                || self.read_backdrop_dot(),
                |offset| Dot::from(color_ram.as_ref().read_hword(offset as u32)),
            )
    }
}
