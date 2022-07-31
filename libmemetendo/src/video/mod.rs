mod bg;
mod obj;
mod reg;
pub mod screen;

use intbits::Bits;
use tinyvec::{array_vec, ArrayVec};

use crate::{
    bus::Bus,
    dma::{self, Dmas},
    irq::{Interrupt, Irq},
    video::reg::Mode,
};

use self::{
    obj::Oam,
    reg::{
        BackgroundAffine, BackgroundControl, BackgroundOffset, BlendCoefficient, BlendControl,
        DisplayControl, DisplayStatus, Mosaic, ReferencePoint, WindowControl, WindowDimensions,
    },
    screen::{FrameBuffer, Rgb, Screen},
};

#[derive(Copy, Clone)]
pub struct PaletteRam([u8; 0x400]);

impl Default for PaletteRam {
    fn default() -> Self {
        Self([0; 0x400])
    }
}

impl Bus for PaletteRam {
    fn read_byte(&mut self, addr: u32) -> u8 {
        self.0.read_byte(addr)
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        // 8-bit writes act weird; write as a hword instead.
        self.0.write_hword(addr, u16::from_le_bytes([value, value]));
    }

    fn write_hword(&mut self, addr: u32, value: u16) {
        self.0.write_hword(addr, value);
    }
}

pub struct VramBus<'a>(&'a mut Video);

impl Bus for VramBus<'_> {
    fn read_byte(&mut self, addr: u32) -> u8 {
        self.0.vram.read_byte(addr)
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        // Like palette RAM, but only write a hword for BG data.
        if (addr as usize) < self.0.dispcnt.obj_vram_offset() {
            self.0
                .vram
                .write_hword(addr, u16::from_le_bytes([value, value]));
        }
    }

    fn write_hword(&mut self, addr: u32, value: u16) {
        self.0.vram.write_hword(addr, value);
    }
}

pub const HORIZ_DOTS: u16 = 308;
pub const VERT_DOTS: u8 = 228;

pub const HBLANK_DOT: u8 = 240;
pub const VBLANK_DOT: u8 = 160;

#[derive(Clone)]
pub struct Video {
    x: u16,
    y: u8,
    cycle_accum: u32,
    tile_mode_bg_order: ArrayVec<[usize; 4]>,
    frame_buf: FrameBuffer,

    vram: Box<[u8]>,
    pub palette_ram: PaletteRam,
    pub oam: Oam,

    dispcnt: DisplayControl,
    pub dispstat: DisplayStatus,
    pub greenswp: u16,
    bgcnt: [BackgroundControl; 4],
    pub bgofs: [BackgroundOffset; 4],
    pub bgref: [ReferencePoint; 2],
    pub bgp: [BackgroundAffine; 2],
    pub win: [WindowDimensions; 2],
    pub winin: [WindowControl; 2],
    pub winout: WindowControl,
    pub winobj: WindowControl,
    pub mosaic_bg: Mosaic,
    pub mosaic_obj: Mosaic,
    pub bldcnt: BlendControl,
    pub bldalpha: (BlendCoefficient, BlendCoefficient),
    pub bldy: BlendCoefficient,
}

impl Default for Video {
    fn default() -> Self {
        Self::new()
    }
}

impl Video {
    #[must_use]
    pub fn new() -> Self {
        Self {
            x: 0,
            y: 0,
            cycle_accum: 0,
            tile_mode_bg_order: array_vec![0, 1, 2, 3],
            frame_buf: FrameBuffer::new(),
            vram: vec![0; 0x1_8000].into_boxed_slice(),
            palette_ram: PaletteRam::default(),
            oam: Oam::default(),
            dispcnt: DisplayControl::default(),
            dispstat: DisplayStatus::default(),
            greenswp: 0,
            bgcnt: [BackgroundControl::default(); 4],
            bgofs: [BackgroundOffset::default(); 4],
            bgref: [ReferencePoint::default(); 2],
            bgp: [BackgroundAffine::default(); 2],
            win: [WindowDimensions::default(); 2],
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
    pub fn step(&mut self, screen: &mut impl Screen, irq: &mut Irq, dmas: &mut Dmas, cycles: u32) {
        self.cycle_accum += cycles;
        while self.cycle_accum >= 4 {
            self.cycle_accum -= 4;
            if self.x < HBLANK_DOT.into() && self.y < VBLANK_DOT {
                let rgb = self.compute_rgb();
                self.frame_buf
                    .set_pixel(self.x.into(), self.y.into(), rgb, self.greenswp.bit(0));
            }

            self.x += 1;
            if self.x == HBLANK_DOT.into() {
                if self.dispstat.hblank_irq_enabled {
                    irq.request(Interrupt::HBlank);
                }
                if self.y < VBLANK_DOT {
                    dmas.notify(dma::Event::HBlank);
                }

                if self.y < VBLANK_DOT - 1 {
                    for (i, bg_ref) in self.bgref.iter_mut().enumerate() {
                        bg_ref.internal.0 += i32::from(self.bgp[i].b);
                        bg_ref.internal.1 += i32::from(self.bgp[i].d);
                    }
                }
                if self.y == VBLANK_DOT - 1 {
                    screen.present_frame(&self.frame_buf);
                }
            }
            if self.x >= HORIZ_DOTS {
                self.x = 0;
                self.y += 1;
                if self.y >= VERT_DOTS {
                    self.y = 0;
                } else if self.y == VBLANK_DOT {
                    if self.dispstat.vblank_irq_enabled {
                        irq.request(Interrupt::VBlank);
                    }
                    dmas.notify(dma::Event::VBlank);

                    for bg_ref in &mut self.bgref {
                        bg_ref.internal = bg_ref.external();
                    }
                }

                if self.dispstat.vcount_irq_enabled && self.y == self.dispstat.vcount_target {
                    irq.request(Interrupt::VCount);
                }
            }
        }
    }

    pub fn set_dispcnt_lo_bits(&mut self, bits: u8) {
        let old_mode = self.dispcnt.mode();
        self.dispcnt.set_lo_bits(bits);
        if old_mode != self.dispcnt.mode() && self.dispcnt.mode_type() == Mode::Tile {
            self.tile_mode_bg_order = match self.dispcnt.mode() {
                0 => array_vec![0, 1, 2, 3],
                1 => array_vec![0, 1, 2],
                2 => array_vec![2, 3],
                _ => unreachable!(),
            };
            self.priority_sort_tile_mode_bgs();
        }
    }

    pub fn set_dispcnt_hi_bits(&mut self, bits: u8) {
        self.dispcnt.set_hi_bits(bits);
    }

    #[must_use]
    pub fn dispcnt(&self) -> &DisplayControl {
        &self.dispcnt
    }

    #[must_use]
    pub fn dispstat_lo_bits(&self) -> u8 {
        self.dispstat.lo_bits(
            self.y >= VBLANK_DOT && self.y != 227,
            self.x >= HBLANK_DOT.into(),
            self.y,
        )
    }

    #[must_use]
    pub fn vcount(&self) -> u8 {
        self.y
    }

    #[must_use]
    pub fn vram(&mut self) -> VramBus {
        VramBus(self)
    }
}

#[derive(Debug, Copy, Clone)]
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

#[derive(Debug, Copy, Clone)]
enum DotInfo {
    Object(obj::DotInfo),
    Background(bg::DotInfo),
    Backdrop,
}

impl Video {
    fn compute_rgb(&mut self) -> Rgb {
        if self.dispcnt.forced_blank {
            return WHITE_DOT.to_rgb();
        }

        // TODO: mosaics
        let top_win = self.find_top_window();
        let top_infos = self.compute_top_dots(top_win);
        let top_dot = self.read_dot(top_infos[0]);

        let obj_alpha_mode = matches!(
            top_infos[0],
            DotInfo::Object(obj::DotInfo {
                mode: obj::Mode::AlphaBlend,
                ..
            })
        );
        let is_target = |top_dot_idx: usize| {
            let targeted = match top_infos[top_dot_idx] {
                DotInfo::Object(_) => self.bldcnt.obj_target[top_dot_idx],
                DotInfo::Background(bg) => self.bldcnt.bg_target[top_dot_idx][bg.index()],
                DotInfo::Backdrop => self.bldcnt.backdrop_target[top_dot_idx],
            };
            let win_blendfx = self
                .window_control(top_win)
                .map_or(true, |w| w.blendfx_enabled);

            targeted && win_blendfx
        };

        let dot = match self.bldcnt.mode() {
            _ if !is_target(0) => top_dot,
            mode if is_target(1) && (mode == 1 || obj_alpha_mode) => {
                let bot_dot = self.read_dot(top_infos[1]);
                self.alpha_blend_dots(top_dot, bot_dot)
            }
            0 | 1 => top_dot,
            2 => self.brighten_dot(false, top_dot),
            3 => self.brighten_dot(true, top_dot),
            _ => unreachable!(),
        };

        dot.to_rgb()
    }

    fn read_dot(&mut self, info: DotInfo) -> Dot {
        let mut palette_ram = |offset| Dot::from(self.palette_ram.read_hword(offset));
        let vram = |offset| Dot::from(self.vram.as_ref().read_hword(offset));

        match info {
            DotInfo::Object(info) => palette_ram(0x200 + info.palette.ram_offset()),
            DotInfo::Background(info) => match info {
                bg::DotInfo::TileMode { palette, .. } => palette_ram(palette.ram_offset()),
                bg::DotInfo::Mode3 { pos: (x, y) } => vram(2 * (y * u32::from(HBLANK_DOT) + x)),
                bg::DotInfo::Mode4 { color_idx } => palette_ram(2 * u32::from(color_idx)),
                #[allow(clippy::cast_possible_truncation)]
                bg::DotInfo::Mode5 { pos: (x, y) } => {
                    vram(self.dispcnt.frame_vram_offset() as u32 + 2 * (y * 160 + x))
                }
            },
            DotInfo::Backdrop => palette_ram(0),
        }
    }

    fn compute_top_dots(&mut self, top_win: Window) -> [DotInfo; 2] {
        let mut obj_info = self.compute_top_obj_dot(top_win);

        match self.dispcnt.mode_type() {
            Mode::Tile => {
                let mut infos = [DotInfo::Backdrop; 2];
                let mut bg_iter = self.compute_bg_tile_mode_dot_iter(top_win).peekable();

                while let DotInfo::Backdrop = infos[1] {
                    let top = match (obj_info, bg_iter.peek()) {
                        (Some(obj), Some(bg))
                            if obj.priority_over_bg <= self.bgcnt[bg.index()].priority() =>
                        {
                            DotInfo::Object(obj_info.take().unwrap())
                        }
                        (Some(_) | None, Some(_)) => DotInfo::Background(bg_iter.next().unwrap()),
                        (Some(_), None) => DotInfo::Object(obj_info.take().unwrap()),
                        (None, None) => break,
                    };

                    if let DotInfo::Backdrop = infos[0] {
                        infos[0] = top;
                    } else {
                        infos[1] = top;
                    }
                }

                infos
            }
            Mode::Bitmap => {
                let bg_info = self.compute_bg_bitmap_mode_dot(top_win);

                match (obj_info, bg_info) {
                    (Some(obj), Some(bg))
                        if obj.priority_over_bg <= self.bgcnt[bg.index()].priority() =>
                    {
                        [DotInfo::Object(obj), DotInfo::Background(bg)]
                    }
                    (Some(obj), Some(bg)) => [DotInfo::Background(bg), DotInfo::Object(obj)],
                    (Some(obj), None) => [DotInfo::Object(obj), DotInfo::Backdrop],
                    (None, Some(bg)) => [DotInfo::Background(bg), DotInfo::Backdrop],
                    (None, None) => [DotInfo::Backdrop; 2],
                }
            }
            Mode::Invalid => [DotInfo::Backdrop; 2],
        }
    }

    fn alpha_blend_dots(&self, top: Dot, bot: Dot) -> Dot {
        let factor = (self.bldalpha.0.factor(), self.bldalpha.1.factor());
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let blend = |top: u8, bot: u8| {
            31.min((f32::from(top) * factor.0 + f32::from(bot) * factor.1) as u32) as u8
        };

        Dot {
            r: blend(top.r, bot.r),
            g: blend(top.g, bot.g),
            b: blend(top.b, bot.b),
        }
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
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum Window {
    None,
    Inside0,
    Inside1,
    Object,
    Outside,
}

impl Video {
    fn window_control(&self, win: Window) -> Option<&WindowControl> {
        match win {
            Window::None => None,
            Window::Inside0 => Some(&self.winin[0]),
            Window::Inside1 => Some(&self.winin[1]),
            Window::Object => Some(&self.winobj),
            Window::Outside => Some(&self.winout),
        }
    }

    fn find_top_window(&self) -> Window {
        if self.dispcnt.display_bg_window == [false; 2] && !self.dispcnt.display_obj_window {
            return Window::None;
        }

        for win_idx in 0..2 {
            if !self.dispcnt.display_bg_window[win_idx] {
                continue;
            }

            let (win_x, win_y) = (self.win[win_idx].horiz(), self.win[win_idx].vert());
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
                return match win_idx {
                    0 => Window::Inside0,
                    1 => Window::Inside1,
                    _ => unreachable!(),
                };
            }
        }

        if self.dispcnt.display_obj_window && self.check_dot_inside_obj_window() {
            Window::Object
        } else {
            Window::Outside
        }
    }
}

#[derive(Debug, Copy, Clone)]
struct DotPaletteInfo {
    idx: Option<u16>,
    color_idx: u32,
}

impl DotPaletteInfo {
    fn ram_offset(self) -> u32 {
        let color_idx = if let Some(palette_idx) = self.idx {
            16 * u32::from(palette_idx) + self.color_idx
        } else {
            self.color_idx
        };

        2 * color_idx
    }
}

const TILE_DOT_LEN: u8 = 8;

impl Video {
    #[allow(clippy::similar_names)]
    fn affine_transform_pos(
        (ref_x, ref_y): (i32, i32),
        (dmx, dmy): (i32, i32),
        (dx, dy): (i32, i32),
        (x, y): (i32, i32),
    ) -> (i32, i32) {
        (
            ref_x.wrapping_add(y * dmx).wrapping_add(x * dx) >> 8,
            ref_y.wrapping_add(y * dmy).wrapping_add(x * dy) >> 8,
        )
    }

    fn flip_tile_dot_pos(
        (flip_x, flip_y): (bool, bool),
        (tile_width, tile_height): (u8, u8),
        (mut dot_x, mut dot_y): (u8, u8),
    ) -> (u8, u8) {
        if flip_x {
            dot_x = tile_width * TILE_DOT_LEN - 1 - dot_x;
        }
        if flip_y {
            dot_y = tile_height * TILE_DOT_LEN - 1 - dot_y;
        }

        (dot_x, dot_y)
    }

    fn read_tile_dot_palette(
        &self,
        palette_idx: Option<u16>,
        dot_offset: usize,
        dot_x: u8,
    ) -> Option<DotPaletteInfo> {
        let palette_color_idx = if palette_idx.is_some() {
            u32::from(self.vram[dot_offset] >> (4 * (dot_x % 2))).bits(..4)
        } else {
            u32::from(self.vram[dot_offset])
        };

        (palette_color_idx != 0).then_some(DotPaletteInfo {
            idx: palette_idx,
            color_idx: palette_color_idx,
        })
    }
}
