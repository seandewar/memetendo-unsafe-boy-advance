use crate::{
    arm7tdmi::Cpu,
    bus::GbaBus,
    cart::Cartridge,
    video::{Screen, VideoController},
};

pub struct Gba<'a> {
    cpu: Cpu,
    iwram: Box<[u8]>,
    ewram: Box<[u8]>,
    video: VideoController,
    cart: &'a Cartridge,
}

// A member fn would be nicer, but using &mut self over $gba unnecessarily mutably borrows the
// *whole* Gba struct.
macro_rules! bus {
    ($gba:ident) => {{
        GbaBus {
            iwram: &mut $gba.iwram,
            ewram: &mut $gba.ewram,
            video: &mut $gba.video,
            cart: &$gba.cart,
        }
    }};
}

impl<'a> Gba<'a> {
    pub fn new(cart: &'a Cartridge) -> Self {
        Self {
            cpu: Cpu::new(),
            iwram: vec![0; 0x8000].into_boxed_slice(),
            ewram: vec![0; 0x4_0000].into_boxed_slice(),
            video: VideoController::new(),
            cart,
        }
    }

    pub fn reset(&mut self) {
        let bus = &bus!(self);
        self.cpu.reset(bus);
        self.cpu.skip_bios(bus); // TODO
    }

    pub fn step(&mut self, screen: &mut impl Screen) {
        self.cpu.step(&mut bus!(self));
        self.video.step(screen, 8);
    }

    pub fn dump_ewram(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        std::fs::write(path, &self.ewram[..])
    }
}
