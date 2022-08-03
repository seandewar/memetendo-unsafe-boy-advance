use image::RgbImage;
use libmemetendo::{
    gba::Gba,
    rom::{Bios, Cartridge, Rom},
    video::screen::{self, FrameBuffer},
};
use once_cell::sync::Lazy;

static BIOS_ROM: Lazy<Rom> = Lazy::new(|| {
    Rom::from_file("tests/bios.bin").expect(
        "failed to read BIOS ROM; place it in a \"bios.bin\" file within the tests directory",
    )
});

pub struct Runner<'c> {
    pub gba: Gba<'static, 'c>,
    pub screen: Screen,
}

impl<'c> Runner<'c> {
    pub fn new(test_rom: &'c Rom) -> Self {
        let bios = Bios::new(&BIOS_ROM).expect("bad BIOS ROM");
        let cart = Cartridge::new(test_rom).expect("bad test ROM");

        let mut gba = Gba::new(bios, cart);
        gba.reset(true);

        Self {
            gba,
            screen: Screen::new(),
        }
    }

    pub fn step(&mut self) {
        self.gba.step(&mut self.screen, false);
    }

    pub fn step_frame(&mut self) {
        self.screen.is_new_frame = false;
        while !self.screen.is_new_frame {
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
    is_new_frame: bool,
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
            is_new_frame: false,
        }
    }
}

impl screen::Screen for Screen {
    fn present_frame(&mut self, frame_buf: &FrameBuffer) {
        self.image
            .as_flat_samples_mut()
            .as_mut_slice()
            .copy_from_slice(&frame_buf.0[..]);
        self.is_new_frame = true;
    }
}
