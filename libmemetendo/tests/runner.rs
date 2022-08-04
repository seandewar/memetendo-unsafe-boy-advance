use image::RgbImage;
use libmemetendo::{
    audio,
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

struct NullAudioDevice;

impl audio::Device for NullAudioDevice {
    fn push_sample(&mut self, _sample: (i16, i16)) {}
}

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
        self.gba.step(&mut self.screen, &mut NullAudioDevice, false);
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
