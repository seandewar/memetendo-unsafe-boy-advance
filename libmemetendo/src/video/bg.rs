use intbits::Bits;

use crate::bus::Bus;

use super::{DotPaletteInfo, Video, Window, HBLANK_DOT, TILE_DOT_LEN, VBLANK_DOT};

#[derive(Debug, Copy, Clone)]
pub(super) enum DotInfo {
    TileMode { idx: usize, palette: DotPaletteInfo },
    Mode3 { pos: (u32, u32) },
    Mode4 { color_idx: u8 },
    Mode5 { pos: (u32, u32) },
}

impl DotInfo {
    pub fn index(self) -> usize {
        match self {
            DotInfo::TileMode { idx, .. } => idx,
            DotInfo::Mode3 { .. } | DotInfo::Mode4 { .. } | DotInfo::Mode5 { .. } => 2,
        }
    }
}

impl Video {
    pub(super) fn priority_sort_tile_mode_bgs(&mut self) {
        // If many BGs share the same priority, the one with the smallest index wins.
        self.tile_mode_bg_order.sort_unstable_by(|&a, &b| {
            self.bgcnt[a]
                .priority
                .cmp(&self.bgcnt[b].priority)
                .then_with(|| a.cmp(&b))
        });
    }

    pub(super) fn compute_bg_tile_mode_dot_iter(
        &self,
        win: Window,
    ) -> impl Iterator<Item = DotInfo> + '_ {
        self.tile_mode_bg_order
            .iter()
            .filter(move |&&i| {
                self.dispcnt.display_bg[i]
                    && self.window_control(win).map_or(true, |w| w.display_bg[i])
            })
            .filter_map(|&i| self.compute_bg_tile_mode_dot(i))
    }

    fn compute_bg_tile_mode_dot(&self, bg_idx: usize) -> Option<DotInfo> {
        let text_mode = self.dispcnt.mode == 0 || bg_idx < 2;
        let (x, y) = if text_mode {
            let (x, y) = (i32::from(self.x), i32::from(self.y));
            let (scroll_x, scroll_y) = self.bgofs[bg_idx].get();

            (i32::from(scroll_x) + x, i32::from(scroll_y) + y)
        } else {
            // Unlike text mode, this may result in a negative position.
            self.bg_affine_transform_pos(bg_idx, self.x.into())
        };

        let (tile_x, tile_y) = (
            x.div_euclid(TILE_DOT_LEN.into()),
            y.div_euclid(TILE_DOT_LEN.into()),
        );
        let screen_tile_len = self.bgcnt[bg_idx].screen_tile_len(text_mode);
        let screen_idx = if text_mode {
            self.bgcnt[bg_idx].text_mode_screen_index((
                tile_x / i32::from(screen_tile_len),
                tile_y / i32::from(screen_tile_len),
            ))
        } else {
            0
        };
        let screen_base_offset = self.bgcnt[bg_idx].screen_vram_offset(screen_idx);
        let screen_wraparound = text_mode || self.bgcnt[bg_idx].wraparound;

        let (screen_tile_x, screen_tile_y) = if screen_wraparound {
            (
                tile_x.rem_euclid(screen_tile_len.into()) as u32,
                tile_y.rem_euclid(screen_tile_len.into()) as u32,
            )
        } else {
            if !(0..i32::from(screen_tile_len)).contains(&tile_x)
                || !(0..i32::from(screen_tile_len)).contains(&tile_y)
            {
                return None; // Screen wraparound disabled and out of bounds
            }

            #[allow(clippy::cast_sign_loss)]
            (tile_x as u32, tile_y as u32)
        };
        let screen_tile_idx = screen_tile_y * u32::from(screen_tile_len) + screen_tile_x;

        #[allow(clippy::cast_possible_truncation)]
        let (mut dot_x, mut dot_y) = (
            x.rem_euclid(TILE_DOT_LEN.into()) as u8,
            y.rem_euclid(TILE_DOT_LEN.into()) as u8,
        );
        let (dots_idx, palette_idx) = if text_mode {
            #[allow(clippy::cast_possible_truncation)]
            let tile_info_offset = screen_base_offset as u32 + 2 * screen_tile_idx;
            let tile_info = self.vram.as_ref().read_hword(tile_info_offset);
            let dots_idx = usize::from(tile_info.bits(..10));

            if self.dispcnt.mode == 0 || (self.dispcnt.mode == 1 && bg_idx < 2) {
                (dot_x, dot_y) = Self::flip_tile_dot_pos(
                    (tile_info.bit(10), tile_info.bit(11)),
                    (1, 1),
                    (dot_x, dot_y),
                );
            }
            let color256 = self.bgcnt[bg_idx].color256
                || (self.dispcnt.mode == 1 && bg_idx == 2)
                || self.dispcnt.mode == 2;

            (dots_idx, (!color256).then_some(tile_info.bits(12..)))
        } else {
            let dots_idx_offset = screen_base_offset + screen_tile_idx as usize;
            if dots_idx_offset >= self.vram.len() {
                return None;
            }

            (usize::from(self.vram[dots_idx_offset]), None)
        };

        let color256 = palette_idx.is_none();
        let dot_offset = self.bgcnt[bg_idx].dots_vram_offset()
            + if color256 { 64 } else { 32 } * dots_idx
            + (8 * usize::from(dot_y) + usize::from(dot_x)) / if color256 { 1 } else { 2 };
        if dot_offset >= self.vram.len() {
            return None;
        }

        self.read_tile_dot_palette(palette_idx, dot_offset, dot_x)
            .map(|palette| DotInfo::TileMode {
                idx: bg_idx,
                palette,
            })
    }

    pub(super) fn compute_bg_bitmap_mode_dot(&self, win: Window) -> Option<DotInfo> {
        if !self.dispcnt.display_bg[2] || self.window_control(win).is_some_and(|w| !w.display_bg[2])
        {
            return None;
        }

        let (x, y) = self.bg_affine_transform_pos(2, self.x.into());
        if x < 0 || y < 0 {
            return None;
        }
        #[allow(clippy::cast_sign_loss)]
        let (x, y) = (x as u32, y as u32);

        match self.dispcnt.mode {
            _ if x >= HBLANK_DOT.into() || y >= VBLANK_DOT.into() => None,
            3 => Some(DotInfo::Mode3 { pos: (x, y) }),
            4 => {
                let (x, y) = (x as usize, y as usize);
                let frame_offset = self.dispcnt.frame_vram_offset();
                let color_idx = self.vram[frame_offset + y * usize::from(HBLANK_DOT) + x];

                (color_idx > 0).then_some(DotInfo::Mode4 { color_idx })
            }
            5 if x >= 160 || y >= 128 => None,
            5 => Some(DotInfo::Mode5 { pos: (x, y) }),
            _ => unreachable!(),
        }
    }

    fn bg_affine_transform_pos(&self, bg_idx: usize, x: i32) -> (i32, i32) {
        let params = &self.bgp[bg_idx - 2];
        let d = (i32::from(params.a), i32::from(params.c));

        Self::affine_transform_pos(self.bgref[bg_idx - 2].internal, (0, 0), d, (x, 0))
    }
}
