use std::{fs, rc::Rc};

use image::RgbImage;
use libmemetendo::{
    audio, bios,
    cart::{self, Cartridge},
    gba::Gba,
    video::screen::{self, FrameBuffer},
};

thread_local! {
    static BIOS_ROM: bios::Rom = {
        let buf = fs::read("tests/bios.bin").expect(
            "failed to read BIOS ROM; place it in a \"bios.bin\" file within the tests directory",
        );
        bios::Rom::new(Rc::from(buf)).expect("bad BIOS ROM")
    };
}

pub struct Runner {
    pub gba: Gba,
    pub screen: Screen,
}

impl Runner {
    pub fn new(test_rom: cart::Rom) -> Self {
        let mut gba = Gba::new(BIOS_ROM.with(bios::Rom::clone), Cartridge::from(test_rom));
        gba.reset(true);

        Self {
            gba,
            screen: Screen::new(),
        }
    }

    pub fn step(&mut self) {
        self.gba
            .step(&mut self.screen, &mut audio::NullCallback, false);
    }

    pub fn step_frame(&mut self) {
        self.screen.new_frame = false;
        while !self.screen.new_frame {
            self.step();
        }
    }

    #[allow(unused)]
    pub fn step_for(&mut self, steps: u32) {
        for _ in 0..steps {
            self.step();
        }
    }

    #[allow(unused)]
    pub fn step_frames(&mut self, frames: u32) {
        for _ in 0..frames {
            self.step_frame();
        }
    }
}

pub struct Screen {
    pub image: RgbImage,
    new_frame: bool,
}

impl Default for Screen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen {
    fn new() -> Self {
        Self {
            image: RgbImage::new(screen::WIDTH as u32, screen::HEIGHT as u32),
            new_frame: false,
        }
    }
}

impl screen::Screen for Screen {
    fn finished_frame(&mut self, frame: &FrameBuffer) {
        self.image
            .as_flat_samples_mut()
            .as_mut_slice()
            .copy_from_slice(&frame.0);
        self.new_frame = true;
    }
}
