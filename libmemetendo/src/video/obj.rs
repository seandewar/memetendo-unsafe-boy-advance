use intbits::Bits;
use strum_macros::FromRepr;
use tinyvec::ArrayVec;

use crate::{
    arbitrary_sign_extend,
    bus::Bus,
    video::{HBLANK_DOT, VBLANK_DOT},
};

use super::{screen, DotPaletteInfo, Video, Window, TILE_DOT_LEN};

#[derive(Debug, Copy, Clone)]
enum AffineAttribute {
    Enabled { double_size: bool, params_idx: u8 },
    Disabled { hidden: bool, flip: (bool, bool) },
}

impl Default for AffineAttribute {
    fn default() -> Self {
        Self::Disabled {
            hidden: false,
            flip: (false, false),
        }
    }
}

#[derive(Debug, Default, Copy, Clone)]
struct Attributes {
    pos: (i16, i16),
    affine: AffineAttribute,
    mode: Option<Mode>,
    _mosaic: bool, // TODO
    shape: u8,
    size: u8,
    dots_base_idx: u16,
    priority: u8,
    palette_idx: Option<u16>,
}

impl Attributes {
    fn tiles_size(&self) -> (u8, u8) {
        let tile_sizes = match self.shape {
            0 => [(1, 1), (2, 2), (4, 4), (8, 8)],
            1 => [(2, 1), (4, 1), (4, 2), (8, 4)],
            2 => [(1, 2), (1, 4), (2, 4), (4, 8)],
            3 => return (0, 0),
            _ => unreachable!(),
        };

        tile_sizes[usize::from(self.size)]
    }

    fn is_enabled(&self) -> bool {
        !matches!(self.affine, AffineAttribute::Disabled { hidden: true, .. })
            && self.mode.is_some()
            && self.tiles_size() != (0, 0)
    }

    fn is_double_size(&self) -> bool {
        matches!(
            self.affine,
            AffineAttribute::Enabled {
                double_size: true,
                ..
            }
        )
    }

    fn clip_dots_size(&self) -> (u8, u8) {
        let (width, height) = self.tiles_size();
        let size_mul = if self.is_double_size() { 2 } else { 1 };

        (
            width * TILE_DOT_LEN * size_mul,
            height * TILE_DOT_LEN * size_mul,
        )
    }
}

#[derive(Clone)]
pub struct Oam {
    buf: [u8; 0x400],

    // Split the screen into "regions" and cache attribute info to optimize object drawing.
    // This allows us to only consider drawing objects within the region that a dot belongs to,
    // rather than needing to check all 128 potential objects per dot.
    attrs: [Attributes; 128],
    regions: Box<[ArrayVec<[u8; 128]>]>,
}

const REGIONS_SIZE: (usize, usize) = (
    screen::WIDTH / TILE_DOT_LEN as usize,
    screen::HEIGHT / TILE_DOT_LEN as usize,
);

impl Default for Oam {
    fn default() -> Self {
        let mut oam = Self {
            buf: [0; 0x400],
            attrs: [Attributes::default(); 128],
            regions: vec![ArrayVec::new(); REGIONS_SIZE.0 * REGIONS_SIZE.1].into_boxed_slice(),
        };
        for idx in 0..128 {
            oam.update_cached_attrs(idx, true);
        }

        oam
    }
}

impl Oam {
    fn region_pos((x, y): (u16, u16)) -> (u16, u16) {
        (x / u16::from(TILE_DOT_LEN), y / u16::from(TILE_DOT_LEN))
    }

    fn region_index((region_x, region_y): (u16, u16)) -> usize {
        usize::from(region_y) * REGIONS_SIZE.0 + usize::from(region_x)
    }

    fn update_cached_attrs(&mut self, idx: u8, force_region_update: bool) {
        let update_regions = |regions: &mut [ArrayVec<[u8; 128]>], attrs: &Attributes, remove| {
            if !attrs.is_enabled() {
                return;
            }

            let (clip_width, clip_height) = attrs.clip_dots_size();
            let (start_x, start_y) = attrs.pos;
            let (end_x, end_y) = (
                start_x + i16::from(clip_width) - 1,
                start_y + i16::from(clip_height) - 1,
            );
            if start_x >= HBLANK_DOT.into()
                || start_y >= VBLANK_DOT.into()
                || end_x < 0
                || end_y < 0
            {
                return; // Fully outside of the drawable area; no region.
            }

            #[allow(clippy::cast_sign_loss)]
            let (start_region_x, start_region_y) =
                Self::region_pos((start_x.max(0) as u16, start_y.max(0) as u16));
            #[allow(clippy::cast_sign_loss)]
            let (end_region_x, end_region_y) = Self::region_pos((
                end_x.min(i16::from(HBLANK_DOT) - 1) as u16,
                end_y.min(i16::from(VBLANK_DOT) - 1) as u16,
            ));

            let cmp = |&i: &u8| {
                self.attrs[usize::from(i)]
                    .priority
                    .cmp(&attrs.priority)
                    .then_with(|| i.cmp(&idx))
            };

            let mut region_y = start_region_y;
            while region_y <= end_region_y {
                let mut region_x = start_region_x;
                while region_x <= end_region_x {
                    let region_idxs = &mut regions[Self::region_index((region_x, region_y))];
                    if remove {
                        if let Ok(i) = region_idxs.binary_search_by(cmp) {
                            region_idxs.remove(i);
                        }
                    } else if let Err(i) = region_idxs.binary_search_by(cmp) {
                        region_idxs.insert(i, idx);
                    }

                    region_x += 1;
                }
                region_y += 1;
            }
        };

        let new_attrs = self.read_attributes(idx);
        let old_attrs = &self.attrs[usize::from(idx)];
        let regions_maybe_stale = old_attrs.is_enabled() != new_attrs.is_enabled()
            || old_attrs.is_double_size() != new_attrs.is_double_size()
            || old_attrs.pos != new_attrs.pos
            || old_attrs.shape != new_attrs.shape
            || old_attrs.size != new_attrs.size
            || old_attrs.priority != new_attrs.priority;

        if force_region_update || regions_maybe_stale {
            update_regions(&mut self.regions, old_attrs, true);
            update_regions(&mut self.regions, &new_attrs, false);
        }

        self.attrs[usize::from(idx)] = new_attrs;
    }
}

const OAM_ENTRY_STRIDE: u32 = 8;

impl Bus for Oam {
    fn read_byte(&mut self, addr: u32) -> u8 {
        self.buf.read_byte(addr)
    }

    fn write_hword(&mut self, addr: u32, value: u16) {
        // Updating the cache is quite involved; skip if the value's unchanged.
        if self.buf.read_hword(addr) == value {
            return;
        }

        self.buf.write_hword(addr, value);
        #[allow(clippy::cast_possible_truncation)]
        self.update_cached_attrs((addr / OAM_ENTRY_STRIDE) as u8, false);
    }
}

impl Oam {
    fn read_attributes(&self, idx: u8) -> Attributes {
        let offset = u32::from(idx) * OAM_ENTRY_STRIDE;
        let attrs = [
            self.buf.as_ref().read_hword(offset),
            self.buf.as_ref().read_hword(offset + 2),
            self.buf.as_ref().read_hword(offset + 4),
        ];

        let affine = if attrs[0].bit(8) {
            #[allow(clippy::cast_possible_truncation)]
            AffineAttribute::Enabled {
                double_size: attrs[0].bit(9),
                params_idx: attrs[1].bits(9..14) as u8,
            }
        } else {
            AffineAttribute::Disabled {
                hidden: attrs[0].bit(9),
                flip: (attrs[1].bit(12), attrs[1].bit(13)),
            }
        };
        let color256 = attrs[0].bit(13);
        let palette_idx = (!color256).then_some(attrs[2].bits(12..));

        #[allow(clippy::cast_possible_wrap)]
        let mut y = attrs[0].bits(..8) as i16;
        #[allow(clippy::cast_possible_truncation)]
        if y >= VBLANK_DOT.into() {
            y = i16::from(y as i8);
        }

        #[allow(clippy::cast_possible_truncation)]
        Attributes {
            pos: (arbitrary_sign_extend!(i16, attrs[1].bits(..9), 9), y),
            affine,
            mode: Mode::from_repr(attrs[0].bits(10..12).into()),
            _mosaic: attrs[0].bit(12),
            shape: attrs[0].bits(14..) as u8,
            size: attrs[1].bits(14..) as u8,
            dots_base_idx: attrs[2].bits(..10),
            priority: attrs[2].bits(10..12) as u8,
            palette_idx,
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, FromRepr)]
pub(super) enum Mode {
    Normal,
    AlphaBlend,
    WindowMask,
}

#[derive(Debug, Copy, Clone)]
pub(super) struct DotInfo {
    pub mode: Mode,
    pub priority: u8,
    pub palette: DotPaletteInfo,
}

impl Video {
    fn region_attrs_iter(&self) -> impl Iterator<Item = &Attributes> + '_ {
        let region_idx = Oam::region_index(Oam::region_pos((self.x, self.y.into())));

        self.oam.regions[region_idx]
            .into_iter()
            .map(|i| &self.oam.attrs[usize::from(i)])
    }

    pub(super) fn check_inside_obj_window(&self) -> bool {
        self.dispcnt.display_obj
            && self
                .region_attrs_iter()
                .filter(|&attrs| attrs.mode == Some(Mode::WindowMask))
                .find_map(|attrs| self.compute_obj_dot(attrs))
                .is_some()
    }

    pub(super) fn compute_top_obj_dot(&self, win: Window) -> Option<DotInfo> {
        if !self.dispcnt.display_obj || self.window_control(win).map_or(false, |w| !w.display_obj) {
            return None;
        }

        self.region_attrs_iter()
            .filter(|&attrs| attrs.mode.map_or(false, |mode| mode != Mode::WindowMask))
            .find_map(|attrs| self.compute_obj_dot(attrs))
    }

    fn compute_obj_dot(&self, attrs: &Attributes) -> Option<DotInfo> {
        let (tile_width, tile_height) = attrs.tiles_size();
        let (obj_width, obj_height) = (tile_width * TILE_DOT_LEN, tile_height * TILE_DOT_LEN);

        #[allow(clippy::cast_possible_wrap)]
        let (x, y) = (self.x as i16, i16::from(self.y));
        let (obj_x, obj_y) = attrs.pos;
        let (clip_width, clip_height) = attrs.clip_dots_size();
        if !(obj_x..obj_x + i16::from(clip_width)).contains(&x)
            || !(obj_y..obj_y + i16::from(clip_height)).contains(&y)
        {
            return None; // Clipped
        }

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (mut obj_dot_x, mut obj_dot_y) = ((x - obj_x) as u8, (y - obj_y) as u8);
        (obj_dot_x, obj_dot_y) = match attrs.affine {
            AffineAttribute::Enabled {
                double_size,
                params_idx,
            } => {
                let (mut obj_dot_x, mut obj_dot_y) = (i32::from(obj_dot_x), i32::from(obj_dot_y));
                if double_size {
                    obj_dot_x -= i32::from(obj_width / 2);
                    obj_dot_y -= i32::from(obj_height / 2);
                }

                (obj_dot_x, obj_dot_y) = self.obj_affine_transform_pos(
                    params_idx.into(),
                    (tile_width, tile_height),
                    (obj_dot_x, obj_dot_y),
                );
                if !(0..i32::from(obj_width)).contains(&obj_dot_x)
                    || !(0..i32::from(obj_height)).contains(&obj_dot_y)
                {
                    return None; // Out of sprite bounds
                }

                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                (obj_dot_x as u8, obj_dot_y as u8)
            }
            AffineAttribute::Disabled { flip, .. } => {
                Self::flip_tile_dot_pos(flip, (tile_width, tile_height), (obj_dot_x, obj_dot_y))
            }
        };

        let (tile_x, tile_y) = (obj_dot_x / TILE_DOT_LEN, obj_dot_y / TILE_DOT_LEN);
        let color256 = attrs.palette_idx.is_none();
        let dots_row_stride = if self.dispcnt.obj_1d {
            usize::from(tile_width) * if color256 { 2 } else { 1 }
        } else {
            32 // 2D mapping always uses 32x32 tile maps
        };
        let dots_offset = 0x1_0000
            + 32 * (usize::from(attrs.dots_base_idx)
                + usize::from(tile_y) * dots_row_stride
                + usize::from(tile_x) * if color256 { 2 } else { 1 });

        let (dot_x, dot_y) = (obj_dot_x % TILE_DOT_LEN, obj_dot_y % TILE_DOT_LEN);
        let dot_offset = dots_offset
            + (8 * usize::from(dot_y) + usize::from(dot_x)) / if color256 { 1 } else { 2 };
        if dot_offset < self.dispcnt.obj_vram_offset() || dot_offset >= self.vram.len() {
            return None; // Outside of obj VRAM
        }

        self.read_tile_dot_palette(attrs.palette_idx, dot_offset, dot_x)
            .map(|palette| DotInfo {
                mode: attrs.mode.unwrap(),
                priority: attrs.priority,
                palette,
            })
    }

    #[allow(clippy::similar_names)]
    fn obj_affine_transform_pos(
        &self,
        params_idx: u32,
        (tile_width, tile_height): (u8, u8),
        (dot_x, dot_y): (i32, i32),
    ) -> (i32, i32) {
        let params_offset = 6 + 32 * params_idx;
        #[allow(clippy::cast_possible_wrap)]
        let (dx, dmx, dy, dmy) = (
            i32::from(self.oam.buf.as_ref().read_hword(params_offset) as i16),
            i32::from(self.oam.buf.as_ref().read_hword(params_offset + 8) as i16),
            i32::from(self.oam.buf.as_ref().read_hword(params_offset + 16) as i16),
            i32::from(self.oam.buf.as_ref().read_hword(params_offset + 24) as i16),
        );
        let (half_dot_width, half_dot_height) = (
            i32::from(tile_width * TILE_DOT_LEN / 2),
            i32::from(tile_height * TILE_DOT_LEN / 2),
        );

        Self::affine_transform_pos(
            (half_dot_width << 8, half_dot_height << 8),
            (dmx, dmy),
            (dx, dy),
            (dot_x - half_dot_width, dot_y - half_dot_height),
        )
    }
}
