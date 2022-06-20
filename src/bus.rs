#![allow(clippy::module_name_repetitions)]

use intbits::Bits;

pub trait Bus {
    fn read_byte(&self, addr: u32) -> u8;
}

impl Bus for &[u8] {
    fn read_byte(&self, addr: u32) -> u8 {
        self[addr as usize]
    }
}

pub trait BusMut: Bus {
    fn write_byte(&mut self, addr: u32, value: u8);
}

impl Bus for &mut [u8] {
    fn read_byte(&self, addr: u32) -> u8 {
        self[addr as usize]
    }
}

impl BusMut for &mut [u8] {
    fn write_byte(&mut self, addr: u32, value: u8) {
        self[addr as usize] = value;
    }
}

pub trait BusExt {
    fn read_hword(&self, addr: u32) -> u16;
    fn read_word(&self, addr: u32) -> u32;
}

impl<T: Bus> BusExt for T {
    fn read_hword(&self, addr: u32) -> u16 {
        let lo = self.read_byte(addr);
        let hi = self.read_byte(addr.wrapping_add(1));

        u16::from_le_bytes([lo, hi])
    }

    fn read_word(&self, addr: u32) -> u32 {
        let lo = self.read_hword(addr);
        let hi = self.read_hword(addr.wrapping_add(2));

        u32::from(lo).with_bits(16.., hi.into())
    }
}

pub trait BusMutExt {
    fn write_hword(&mut self, addr: u32, value: u16);
    fn write_word(&mut self, addr: u32, value: u32);
}

impl<T: BusMut> BusMutExt for T {
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
}

impl<T: Bus> BusAlignedExt for T {
    fn read_hword_aligned(&self, addr: u32) -> u16 {
        BusExt::read_hword(self, addr & !1)
    }

    fn read_word_aligned(&self, addr: u32) -> u32 {
        BusExt::read_word(self, addr & !0b11)
    }
}

pub trait BusMutAlignedExt {
    fn write_hword_aligned(&mut self, addr: u32, value: u16);
    fn write_word_aligned(&mut self, addr: u32, value: u32);
}

impl<T: BusMut> BusMutAlignedExt for T {
    fn write_hword_aligned(&mut self, addr: u32, value: u16) {
        BusMutExt::write_hword(self, addr & !1, value);
    }

    fn write_word_aligned(&mut self, addr: u32, value: u32) {
        BusMutExt::write_word(self, addr & !0b11, value);
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
    }

    impl BusMut for NullBus {
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
    }

    impl BusMut for VecBus {
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
