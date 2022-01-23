use crate::{arm7tdmi::Cpu, bus::GbaBus};

#[derive(Default, Debug)]
struct Gba {
    cpu: Cpu,
    bus: GbaBus,
}

impl Gba {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn reset(&mut self) {
        self.cpu.reset();
    }

    pub fn step(&mut self, cycles: usize) {
        self.cpu.step(&mut self.bus, cycles);
    }
}
