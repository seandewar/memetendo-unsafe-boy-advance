#[allow(clippy::module_name_repetitions)]
pub trait DataBus {
    #[must_use]
    fn read8(&self, addr: u32) -> u8;

    fn write8(&mut self, addr: u32, value: u8);
}

#[allow(clippy::module_name_repetitions)]
#[derive(Default, Debug)]
pub struct GbaBus;

impl DataBus for GbaBus {
    fn read8(&self, _addr: u32) -> u8 {
        todo!()
    }

    fn write8(&mut self, _addr: u32, _value: u8) {
        todo!()
    }
}
