use crate::{arm7tdmi::Cpu, bus::GbaBus};

#[derive(Default, Debug)]
pub struct Gba {
    cpu: Cpu,
    bus: GbaBus,
}

impl Gba {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.cpu.reset(&self.bus);
    }

    pub fn step(&mut self) {
        self.cpu.step(&mut self.bus);
    }
}
