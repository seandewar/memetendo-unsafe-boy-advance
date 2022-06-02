#![allow(clippy::module_name_repetitions)]

use intbits::Bits;

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

#[derive(Default, Debug)]
pub struct GbaBus;

impl Bus for GbaBus {
    fn read_byte(&self, _addr: u32) -> u8 {
        todo!()
    }

    fn write_byte(&mut self, _addr: u32, _value: u8) {
        todo!()
    }
}

#[cfg(test)]
pub(super) mod tests {
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
    pub struct VecBus(pub Vec<u8>);

    impl Bus for VecBus {
        fn read_byte(&self, addr: u32) -> u8 {
            self.0
                .get(usize::try_from(addr).unwrap())
                .copied()
                .unwrap_or(0xff)
        }

        fn write_byte(&mut self, addr: u32, value: u8) {
            if let Some(v) = self.0.get_mut(usize::try_from(addr).unwrap()) {
                *v = value;
            }
        }
    }
}
