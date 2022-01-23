pub trait DataBus {
    #[must_use]
    fn read8(&self, addr: u32) -> u8;
    fn write8(&mut self, addr: u32, value: u8);
}

#[derive(Default, Debug)]
pub(crate) struct GbaBus;

impl DataBus for GbaBus {
    fn read8(&self, addr: u32) -> u8 {
        todo!()
    }

    fn write8(&mut self, addr: u32, value: u8) {
        todo!()
    }
}
