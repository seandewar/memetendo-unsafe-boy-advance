use crate::{arm7tdmi::Cpu, bus::GbaBus, cart::Cartridge};

#[derive(Debug)]
pub(super) struct ExternalWram(pub [u8; 0x4_0000]);

impl ExternalWram {
    pub(super) fn new() -> Self {
        Self([0; 0x4_0000])
    }
}

#[derive(Debug)]
pub(super) struct InternalWram(pub [u8; 0x8000]);

impl InternalWram {
    pub(super) fn new() -> Self {
        Self([0; 0x8000])
    }
}

#[derive(Debug)]
pub struct Gba<'a> {
    cpu: Cpu,
    iwram: InternalWram,
    ewram: ExternalWram,
    cart: &'a Cartridge,
}

// A member fn would be nicer, but using &mut self over $gba unnecessarily mutably borrows the
// *whole* Gba struct.
macro_rules! bus {
    ($gba:ident) => {{
        GbaBus {
            iwram: &mut $gba.iwram,
            ewram: &mut $gba.ewram,
            cart: &$gba.cart,
        }
    }};
}

impl<'a> Gba<'a> {
    pub fn new(cart: &'a Cartridge) -> Self {
        Self {
            cpu: Cpu::new(),
            iwram: InternalWram::new(),
            ewram: ExternalWram::new(),
            cart,
        }
    }

    pub fn reset(&mut self) {
        let bus = &bus!(self);
        self.cpu.reset(bus);
        self.cpu.skip_bios(bus); // TODO
    }

    pub fn step(&mut self) {
        self.cpu.step(&mut bus!(self));
    }

    pub fn write_fuzz_result(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        std::fs::write(path, &self.ewram.0[..0x40])
    }
}
