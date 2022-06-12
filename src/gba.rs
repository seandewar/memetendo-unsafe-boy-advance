use crate::{
    arm7tdmi::Cpu,
    bus::GbaBus,
    cart::{Bios, Cartridge},
    video::{Screen, VideoController},
};

pub struct Gba<'a, 'b> {
    cpu: Cpu,
    iwram: Box<[u8]>,
    ewram: Box<[u8]>,
    video: VideoController,
    cart: &'a mut Cartridge,
    bios: &'b Bios,
}

// A member fn would be nicer, but using &mut self over $gba unnecessarily mutably borrows the
// *whole* Gba struct.
macro_rules! bus {
    ($gba:ident) => {{
        GbaBus {
            iwram: &mut $gba.iwram,
            ewram: &mut $gba.ewram,
            video: &mut $gba.video,
            cart: &mut $gba.cart,
            bios: &$gba.bios,
        }
    }};
}

impl<'a, 'b> Gba<'a, 'b> {
    pub fn new(bios: &'b Bios, cart: &'a mut Cartridge) -> Self {
        Self {
            cpu: Cpu::new(),
            iwram: vec![0; 0x8000].into_boxed_slice(),
            ewram: vec![0; 0x4_0000].into_boxed_slice(),
            video: VideoController::new(),
            cart,
            bios,
        }
    }

    pub fn reset(&mut self) {
        let bus = &bus!(self);
        self.cpu.reset(bus);
    }

    pub fn reset_and_skip_bios(&mut self) {
        self.reset();
        let bus = &bus!(self);
        self.cpu.skip_bios(bus);

        self.iwram[0x7e00..].fill(0);
    }

    pub fn step(&mut self, screen: &mut impl Screen) {
        self.cpu.step(&mut bus!(self));
        self.video.step(screen, 8);
    }
}
