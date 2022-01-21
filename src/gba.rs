use crate::arm7tdmi;

#[derive(Default, Debug)]
struct Gba {
    cpu: arm7tdmi::Cpu,
}

impl Gba {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn soft_reset(&mut self) {
        self.cpu.soft_reset();
    }

    pub fn hard_reset(&mut self) {
        self.cpu.hard_reset();
    }

    pub fn step(&mut self, cycles: usize) {
        self.cpu.step(cycles);
    }
}
