use crate::arm7tdmi;

#[derive(Default, Debug)]
struct Gba {
    cpu: arm7tdmi::Cpu,
}

impl Gba {
    fn new() -> Self {
        Default::default()
    }

    fn reset(&mut self) {
        self.cpu.reset();
    }
}
