use crate::arm7tdmi;

#[derive(Default, Debug)]
struct Gba {
    cpu: arm7tdmi::Cpu,
}

impl Gba {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn reset(&mut self) {
        self.cpu.reset();
    }

    pub fn step(&mut self, cycles: usize) {
        self.cpu.step(cycles);
    }
}
