#![allow(clippy::module_name_repetitions)]

use intbits::Bits;

use crate::{
    cart::Cartridge,
    gba::{ExternalWram, InternalWram},
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

#[derive(Debug)]
pub(super) struct GbaBus<'a> {
    pub(super) iwram: &'a mut InternalWram,
    pub(super) ewram: &'a mut ExternalWram,
    pub(super) cart: &'a Cartridge,
}

impl GbaBus<'_> {
    fn read_rom(&self, addr: u32) -> u8 {
        self.cart
            .rom()
            .get((addr & 0x01ff_ffff) as usize)
            .copied()
            .unwrap_or(0xff)
    }
}

impl Bus for GbaBus<'_> {
    fn read_byte(&self, addr: u32) -> u8 {
        match addr {
            // BIOS
            0x0000_0000..=0x0000_3fff => 0xff, // TODO
            // External WRAM
            0x0200_0000..=0x0203_ffff => self.ewram.0[(addr & 0x3_ffff) as usize],
            // Internal WRAM
            0x0300_0000..=0x0300_7fff => self.iwram.0[(addr & 0x7fff) as usize],
            // I/O Registers
            0x0400_0000..=0x0400_03fe => 0xff, // TODO
            // Palette RAM
            0x0500_0000..=0x0500_03ff => 0xff, // TODO
            // VRAM
            0x0600_0000..=0x0601_7fff => 0xff, // TODO
            // OAM
            0x0700_0000..=0x0700_03ff => 0xff, // TODO
            // ROM Mirror; TODO: Wait state 0
            0x0800_0000..=0x09ff_ffff => self.read_rom(addr),
            // ROM Mirror; TODO: Wait state 1
            0x0a00_0000..=0x0bff_ffff => self.read_rom(addr),
            // ROM Mirror; TODO: Wait state 2
            0x0c00_0000..=0x0dff_ffff => self.read_rom(addr),
            // SRAM
            0x0e00_0000..=0x0e00_ffff => 0xff, // TODO
            // Unused (TODO: what is the behaviour? Probably open bus)
            _ => 0xff,
        }
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        match addr {
            // External WRAM
            0x0200_0000..=0x0203_ffff => {
                self.ewram.0[(addr & 0x3_ffff) as usize] = value;
                println!("ewram! {}", value);
            }
            // Internal WRAM
            0x0300_0000..=0x0300_7fff => self.iwram.0[(addr & 0x7fff) as usize] = value,
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
