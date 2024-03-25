mod bg;
mod obj;
mod reg;

use std::iter;

use intbits::Bits;
use tinyvec::{array_vec, ArrayVec};

use crate::{
    bus::Bus,
    dma::{self, Dma},
    irq::{Interrupt, Irq},
    video::reg::BackgroundMode,
};

use self::{
    obj::Oam,
    reg::{
        BackgroundAffine, BackgroundControl, BackgroundOffset, BlendCoefficient, BlendControl,
        BlendMode, DisplayControl, DisplayStatus, MosaicSize, ReferencePoint, WindowControl,
        WindowDimensions,
    },
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

pub struct Vram<'a>(&'a mut Video);

impl Vram<'_> {
    fn offset(addr: u32) -> u32 {
        if addr < 0x1_8000 {
            addr
        } else {
            addr & !0xf000
        }
    }
}

impl Bus for Vram<'_> {
    fn read_byte(&mut self, addr: u32) -> u8 {
        self.0.vram.read_byte(Self::offset(addr))
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        // Like palette RAM, but only write a hword for BG data.
        let addr = Self::offset(addr);
        if usize::try_from(addr).unwrap() < self.0.dispcnt.obj_vram_offset() {
            self.0
                .vram
                .write_hword(addr, u16::from_le_bytes([value, value]));
        }
    }

    fn write_hword(&mut self, addr: u32, value: u16) {
        self.0.vram.write_hword(Self::offset(addr), value);
    }
}

#[derive(Clone)]
pub struct Video {
    x: u16,
    y: u8,
    cycle_accum: u16,
    tile_mode_bg_order: ArrayVec<[usize; 4]>,

    vram: Box<[u8]>,
    pub palette_ram: PaletteRam,
    pub oam: Oam,

    dispcnt: DisplayControl,
    dispstat: DisplayStatus,
    greenswp: u16,
    bgcnt: [BackgroundControl; 4],
    bgofs: [BackgroundOffset; 4],
    bgref: [ReferencePoint; 2],
    bgp: [BackgroundAffine; 2],
    win: [WindowDimensions; 2],
    winin: [WindowControl; 2],
    winout: WindowControl,
    winobj: WindowControl,
    mosaic_bg: MosaicSize,
    mosaic_obj: MosaicSize,
    bldcnt: BlendControl,
    bldalpha: (BlendCoefficient, BlendCoefficient),
    bldy: BlendCoefficient,
}

impl Default for Video {
    fn default() -> Self {
        Self::new()
    }
}

pub const HORIZ_DOTS: u16 = 308;
pub const VERT_DOTS: u8 = 228;

pub const HBLANK_DOT: u8 = 240;
pub const VBLANK_DOT: u8 = 160;

impl Video {
    #[must_use]
    pub fn new() -> Self {
        Self {
            x: 0,
            y: 0,
            cycle_accum: 0,
            tile_mode_bg_order: array_vec![0, 1, 2, 3],
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
            mosaic_bg: MosaicSize::default(),
            mosaic_obj: MosaicSize::default(),
            bldcnt: BlendControl::default(),
            bldalpha: (BlendCoefficient::default(), BlendCoefficient::default()),
            bldy: BlendCoefficient::default(),
        }
    }

    // Panic should be impossible as self.x should be < HBLANK_DOT when calling screen.put_dot(),
    // which fits in a u8.
    #[allow(clippy::missing_panics_doc)]
    pub fn step(&mut self, cb: &mut impl Callback, irq: &mut Irq, dma: &mut Dma, cycles: u8) {
        self.cycle_accum += u16::from(cycles);
        while self.cycle_accum >= 4 {
            self.cycle_accum -= 4;

            if self.x < HBLANK_DOT.into() && self.y < VBLANK_DOT && !cb.is_frame_skipping() {
                cb.put_dot(self.x.try_into().unwrap(), self.y, self.compute_dot());
            }

            self.x += 1;
            if self.x == HBLANK_DOT.into() {
                if self.dispstat.hblank_irq_enabled {
                    irq.request(Interrupt::HBlank);
                }
                if self.y < VBLANK_DOT {
                    dma.notify(dma::Event::HBlank);
                }

                if self.y < VBLANK_DOT - 1 {
                    for (i, bg_ref) in self.bgref.iter_mut().enumerate() {
                        bg_ref.internal.0 += i32::from(self.bgp[i].b);
                        bg_ref.internal.1 += i32::from(self.bgp[i].d);
                    }
                }
                if self.y == VBLANK_DOT - 1 {
                    cb.end_frame(self.greenswp.bit(0));
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
                    dma.notify(dma::Event::VBlank);

                    for bg_ref in &mut self.bgref {
                        bg_ref.internal = bg_ref.external;
                    }
                }

                if self.dispstat.vcount_irq_enabled && self.y == self.dispstat.vcount_target {
                    irq.request(Interrupt::VCount);
                }
            }
        }
    }

    #[must_use]
    pub fn vram(&mut self) -> Vram {
        Vram(self)
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Dot {
    r: u8,
    g: u8,
    b: u8,
}

impl From<u16> for Dot {
    fn from(value: u16) -> Self {
        Dot {
            r: value.bits(..5).try_into().unwrap(),
            g: value.bits(5..10).try_into().unwrap(),
            b: value.bits(10..15).try_into().unwrap(),
        }
    }
}

impl Dot {
    pub const MAX_COMPONENT: u8 = 31;
    pub const WHITE: Dot = Dot::new(
        Self::MAX_COMPONENT,
        Self::MAX_COMPONENT,
        Self::MAX_COMPONENT,
    );

    const fn new(r: u8, g: u8, b: u8) -> Dot {
        debug_assert!(
            r <= Self::MAX_COMPONENT && g <= Self::MAX_COMPONENT && b <= Self::MAX_COMPONENT
        );
        Dot { r, g, b }
    }

    #[must_use]
    pub const fn red(self) -> u8 {
        self.r
    }

    #[must_use]
    pub const fn green(self) -> u8 {
        self.g
    }

    #[must_use]
    pub const fn blue(self) -> u8 {
        self.b
    }
}

pub trait Callback {
    fn put_dot(&mut self, x: u8, y: u8, dot: Dot);
    fn end_frame(&mut self, green_swap: bool);
    fn is_frame_skipping(&self) -> bool;
}

#[derive(Debug, Copy, Clone)]
enum DotInfo {
    Object(obj::DotInfo),
    Background(bg::DotInfo),
    Backdrop,
}

impl Video {
    fn compute_dot(&mut self) -> Dot {
        if self.dispcnt.forced_blank {
            return Dot::WHITE;
        }

        let top_win = self.find_top_window();
        let mut top_iter = self.compute_top_dots_iter(top_win).peekable();
        let top_info = top_iter.next().unwrap();
        let top_dot = self.read_dot(top_info);

        let obj_alpha_mode = matches!(
            top_info,
            DotInfo::Object(obj::DotInfo {
                mode: obj::Mode::AlphaBlend,
                ..
            })
        );
        let is_target = |info: &DotInfo, dot_idx: usize| {
            let targeted = match info {
                DotInfo::Object(_) => self.bldcnt.obj_target[dot_idx] || obj_alpha_mode,
                DotInfo::Background(bg) => self.bldcnt.bg_target[dot_idx][bg.index()],
                DotInfo::Backdrop => self.bldcnt.backdrop_target[dot_idx],
            };
            let win_blendfx = self
                .window_control(top_win)
                .map_or(true, |w| w.blendfx_enabled);

            targeted && win_blendfx
        };

        match self.bldcnt.mode {
            _ if !is_target(&top_info, 0) => top_dot,
            mode if (mode == BlendMode::Alpha || obj_alpha_mode)
                && is_target(top_iter.peek().unwrap(), 1) =>
            {
                let bot_dot = self.read_dot(top_iter.next().unwrap());
                self.alpha_blend_dots(top_dot, bot_dot)
            }
            BlendMode::Brighten => self.adjust_dot_brightness(false, top_dot),
            BlendMode::Dim => self.adjust_dot_brightness(true, top_dot),
            _ => top_dot,
        }
    }

    fn read_dot(&self, info: DotInfo) -> Dot {
        let palette_ram = |offset| Dot::from(self.palette_ram.0.as_ref().read_hword(offset));
        let vram = |offset| Dot::from(self.vram.as_ref().read_hword(offset));

        match info {
            DotInfo::Object(info) => palette_ram(0x200 + info.palette.ram_offset()),
            DotInfo::Background(info) => match info {
                bg::DotInfo::TileMode { palette, .. } => palette_ram(palette.ram_offset()),
                bg::DotInfo::Mode3 { pos: (x, y) } => vram(2 * (y * u32::from(HBLANK_DOT) + x)),
                bg::DotInfo::Mode4 { color_idx } => palette_ram(2 * u32::from(color_idx)),
                bg::DotInfo::Mode5 { pos: (x, y) } => vram(
                    u32::try_from(self.dispcnt.frame_vram_offset()).unwrap() + 2 * (y * 160 + x),
                ),
            },
            DotInfo::Backdrop => palette_ram(0),
        }
    }

    fn compute_top_dots_iter(&self, top_win: Window) -> impl Iterator<Item = DotInfo> + '_ {
        let mut obj_info = self.compute_top_obj_dot(top_win);
        let mut bg_tile_mode_iter = self.compute_bg_tile_mode_dot_iter(top_win).peekable();

        iter::from_fn(move || match self.dispcnt.mode() {
            BackgroundMode::Tile => match (obj_info, bg_tile_mode_iter.peek()) {
                (Some(obj), Some(bg)) if obj.priority <= self.bgcnt[bg.index()].priority => {
                    Some(DotInfo::Object(obj_info.take().unwrap()))
                }
                (_, Some(_)) => Some(DotInfo::Background(bg_tile_mode_iter.next().unwrap())),
                (Some(_), None) => Some(DotInfo::Object(obj_info.take().unwrap())),
                (None, None) => Some(DotInfo::Backdrop),
            },

            BackgroundMode::Bitmap => {
                let mut bg_info = self.compute_bg_bitmap_mode_dot(top_win);

                match (obj_info, bg_info) {
                    (Some(obj), Some(bg)) if obj.priority <= self.bgcnt[bg.index()].priority => {
                        Some(DotInfo::Object(obj_info.take().unwrap()))
                    }
                    (_, Some(_)) => Some(DotInfo::Background(bg_info.take().unwrap())),
                    (Some(_), None) => Some(DotInfo::Object(obj_info.take().unwrap())),
                    (None, None) => Some(DotInfo::Backdrop),
                }
            }

            BackgroundMode::Invalid => Some(DotInfo::Backdrop),
        })
    }

    fn alpha_blend_dots(&self, top: Dot, bot: Dot) -> Dot {
        let factor = (self.bldalpha.0.factor(), self.bldalpha.1.factor());
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let blend = |top: u8, bot: u8| {
            u8::try_from(
                u32::from(Dot::MAX_COMPONENT)
                    .min((f32::from(top) * factor.0 + f32::from(bot) * factor.1) as u32),
            )
            .unwrap()
        };

        Dot::new(
            blend(top.r, bot.r),
            blend(top.g, bot.g),
            blend(top.b, bot.b),
        )
    }

    fn adjust_dot_brightness(&self, darken: bool, dot: Dot) -> Dot {
        let mul = if darken {
            |comp| -f32::from(comp)
        } else {
            |comp| f32::from(Dot::MAX_COMPONENT - comp)
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let blend = |comp| (f32::from(comp) + mul(comp) * self.bldy.factor()) as u8;

        Dot::new(blend(dot.r), blend(dot.g), blend(dot.b))
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

            let (win_x, win_y) = (self.win[win_idx].horiz, self.win[win_idx].vert);
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
                return [Window::Inside0, Window::Inside1][win_idx];
            }
        }

        if self.dispcnt.display_obj_window && self.check_inside_obj_window() {
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
