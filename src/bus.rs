#[allow(clippy::module_name_repetitions)]
pub trait DataBus {
    #[must_use]
    fn read_byte(&self, addr: u32) -> u8;

    fn write_byte(&mut self, addr: u32, value: u8);

    #[must_use]
    fn read_hword(&self, addr: u32) -> u16 {
        let lo = self.read_byte(addr);
        let hi = self.read_byte(addr.wrapping_add(1));

        (u16::from(hi) << 8) | u16::from(lo)
    }

    fn write_hword(&mut self, addr: u32, value: u16) {
        self.write_byte(addr, (value & 0xff) as _);
        self.write_byte(addr.wrapping_add(1), (value >> 8) as _);
    }

    #[must_use]
    fn read_word(&self, addr: u32) -> u32 {
        let lo = self.read_hword(addr);
        let hi = self.read_hword(addr.wrapping_add(2));

        (u32::from(hi) << 16) | u32::from(lo)
    }

    fn write_word(&mut self, addr: u32, value: u32) {
        self.write_hword(addr, (value & 0xff) as _);
        self.write_hword(addr.wrapping_add(2), (value >> 16) as _);
    }
}

#[allow(clippy::module_name_repetitions)]
#[derive(Default, Debug)]
pub struct GbaBus;

impl DataBus for GbaBus {
    fn read_byte(&self, _addr: u32) -> u8 {
        todo!()
    }

    fn write_byte(&mut self, _addr: u32, _value: u8) {
        todo!()
    }
}

#[cfg(test)]
pub(super) struct NullBus;

#[cfg(test)]
impl DataBus for NullBus {
    fn read_byte(&self, _addr: u32) -> u8 {
        0
    }

    fn write_byte(&mut self, _addr: u32, _value: u8) {}
}
