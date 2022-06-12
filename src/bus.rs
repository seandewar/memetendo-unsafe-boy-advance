#![allow(clippy::module_name_repetitions)]

use intbits::Bits;

use crate::{
    cart::{Bios, Cartridge},
    video::VideoController,
};

pub trait Bus {
    fn read_byte(&self, addr: u32) -> u8;
    fn write_byte(&mut self, addr: u32, value: u8);
}

pub trait BusExt {
    fn read_hword(&self, addr: u32) -> u16;
    fn read_word(&self, addr: u32) -> u32;

    fn write_hword(&mut self, addr: u32, value: u16);
    fn write_word(&mut self, addr: u32, value: u32);
}

impl<T: Bus> BusExt for T {
    fn read_hword(&self, addr: u32) -> u16 {
        let lo = self.read_byte(addr);
        let hi = self.read_byte(addr.wrapping_add(1));

        u16::from(lo).with_bits(8.., hi.into())
    }

    fn read_word(&self, addr: u32) -> u32 {
        let lo = self.read_hword(addr);
        let hi = self.read_hword(addr.wrapping_add(2));

        u32::from(lo).with_bits(16.., hi.into())
    }

    #[allow(clippy::cast_possible_truncation)]
    fn write_hword(&mut self, addr: u32, value: u16) {
        self.write_byte(addr, value as u8);
        self.write_byte(addr.wrapping_add(1), value.bits(8..) as _);
    }

    #[allow(clippy::cast_possible_truncation)]
    fn write_word(&mut self, addr: u32, value: u32) {
        self.write_hword(addr, value as u16);
        self.write_hword(addr.wrapping_add(2), value.bits(16..) as _);
    }
}

pub trait BusAlignedExt {
    fn read_hword_aligned(&self, addr: u32) -> u16;
    fn read_word_aligned(&self, addr: u32) -> u32;

    fn write_hword_aligned(&mut self, addr: u32, value: u16);
    fn write_word_aligned(&mut self, addr: u32, value: u32);
}

impl<T: Bus> BusAlignedExt for T {
    fn read_hword_aligned(&self, addr: u32) -> u16 {
        BusExt::read_hword(self, addr & !1)
    }

    fn read_word_aligned(&self, addr: u32) -> u32 {
        BusExt::read_word(self, addr & !0b11)
    }

    fn write_hword_aligned(&mut self, addr: u32, value: u16) {
        BusExt::write_hword(self, addr & !1, value);
    }

    fn write_word_aligned(&mut self, addr: u32, value: u32) {
        BusExt::write_word(self, addr & !0b11, value);
    }
}

pub(super) struct GbaBus<'a> {
    pub iwram: &'a mut Box<[u8]>,
    pub ewram: &'a mut Box<[u8]>,
    pub video: &'a mut VideoController,
    pub cart: &'a mut Cartridge,
    pub bios: &'a Bios,
}

impl GbaBus<'_> {
    fn read_rom(&self, addr: u32) -> u8 {
        self.cart
            .rom()
            .get((addr & 0x01ff_ffff) as usize)
            .copied()
            .unwrap_or(0xff)
    }

    fn read_io(&self, addr: u32) -> u8 {
        match addr & 0x3ff {
            // DISPCNT
            0 => self.video.dispcnt.lo_bits(),
            1 => self.video.dispcnt.hi_bits(),
            // Green swap (undocumented)
            #[allow(clippy::cast_possible_truncation)]
            2 => self.video.green_swap as u8,
            #[allow(clippy::cast_possible_truncation)]
            3 => self.video.green_swap.bits(8..) as u8,
            // DISPSTAT
            4 => self.video.dispstat_lo_bits(),
            5 => self.video.dispstat.vcount_target,
            // VCOUNT
            6 => self.video.vcount(),
            7 => 0,
            _ => 0xff,
        }
    }

    fn write_io(&mut self, addr: u32, value: u8) {
        match addr & 0x3ff {
            // DISPCNT
            0 => self.video.dispcnt.set_lo_bits(value),
            1 => self.video.dispcnt.set_hi_bits(value),
            // Green swap (undocumented)
            2 => self.video.green_swap.set_bits(..8, value.into()),
            3 => self.video.green_swap.set_bits(8.., value.into()),
            // DISPSTAT
            4 => self.video.dispstat.set_lo_bits(value),
            5 => self.video.dispstat.vcount_target = value,
            _ => {}
        }
    }
}

impl Bus for GbaBus<'_> {
    fn read_byte(&self, addr: u32) -> u8 {
        match addr {
            // BIOS
            0x0000_0000..=0x0000_3fff => self.bios.rom()[(addr & 0x3fff) as usize],
            // External WRAM
            0x0200_0000..=0x0203_ffff => self.ewram[(addr & 0x3_ffff) as usize],
            // Internal WRAM
            0x0300_0000..=0x0300_7fff => self.iwram[(addr & 0x7fff) as usize],
            // I/O Registers
            0x0400_0000..=0x0400_03fe => self.read_io(addr),
            // Palette RAM
            0x0500_0000..=0x0500_03ff => self.video.palette_ram[(addr & 0x3ff) as usize],
            // VRAM
            0x0600_0000..=0x0601_7fff => self.video.vram[(addr & 0x1_7fff) as usize],
            // OAM
            0x0700_0000..=0x0700_03ff => self.video.oam[(addr & 0x3ff) as usize],
            // ROM Mirror; TODO: Wait state 0
            0x0800_0000..=0x09ff_ffff => self.read_rom(addr),
            // ROM Mirror; TODO: Wait state 1
            0x0a00_0000..=0x0bff_ffff => self.read_rom(addr),
            // ROM Mirror; TODO: Wait state 2
            0x0c00_0000..=0x0dff_ffff => self.read_rom(addr),
            // SRAM
            0x0e00_0000..=0x0e00_ffff => self.cart.sram[(addr & 0xffff) as usize],
            // Unused (TODO: what is the behaviour? Probably open bus)
            _ => 0xff,
        }
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        match addr {
            // External WRAM
            0x0200_0000..=0x0203_ffff => self.ewram[(addr & 0x3_ffff) as usize] = value,
            // Internal WRAM
            0x0300_0000..=0x0300_7fff => self.iwram[(addr & 0x7fff) as usize] = value,
            // I/O Registers
            0x0400_0000..=0x0400_03fe => self.write_io(addr, value),
            // Palette RAM
            0x0500_0000..=0x0500_03ff => self.video.palette_ram[(addr & 0x3ff) as usize] = value,
            // VRAM
            0x0600_0000..=0x0601_7fff => self.video.vram[(addr & 0x1_7fff) as usize] = value,
            // OAM
            0x0700_0000..=0x0700_03ff => self.video.oam[(addr & 0x3ff) as usize] = value,
            // SRAM
            0x0e00_0000..=0x0e00_ffff => self.cart.sram[(addr & 0xffff) as usize] = value,
            // Read-only or Unused
            _ => {}
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use std::cell::Cell;

    use super::*;

    #[derive(Debug)]
    pub struct NullBus;

    impl Bus for NullBus {
        fn read_byte(&self, _addr: u32) -> u8 {
            0
        }

        fn write_byte(&mut self, _addr: u32, _value: u8) {}
    }

    #[derive(Debug)]
    pub struct VecBus {
        buf: Vec<u8>,
        allow_oob: bool,
        did_oob: Cell<bool>,
    }

    impl VecBus {
        pub fn new(len: usize) -> Self {
            Self {
                buf: vec![0; len],
                allow_oob: false,
                did_oob: Cell::new(false),
            }
        }

        pub fn assert_oob(&mut self, f: &impl Fn(&mut Self)) {
            assert!(!self.allow_oob, "cannot call assert_oob recursively");

            self.allow_oob = true;
            self.did_oob.set(false);
            f(self);

            assert!(
                self.did_oob.get(),
                "expected oob VecBus access, but there was none"
            );
            self.allow_oob = false;
        }
    }

    impl Bus for VecBus {
        fn read_byte(&self, addr: u32) -> u8 {
            self.buf.get(addr as usize).copied().unwrap_or_else(|| {
                self.did_oob.set(true);
                assert!(
                    self.allow_oob,
                    "oob VecBus read at address {:#010x} (len {})",
                    addr,
                    self.buf.len()
                );

                0xaa
            })
        }

        fn write_byte(&mut self, addr: u32, value: u8) {
            if let Some(v) = self.buf.get_mut(addr as usize) {
                *v = value;
            } else {
                self.did_oob.set(true);
                assert!(
                    self.allow_oob,
                    "oob VecBus write at address {:#010x} (value {}, len {})",
                    addr,
                    value,
                    self.buf.len()
                );
            }
        }
    }
}
