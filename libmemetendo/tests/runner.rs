use std::{fs, rc::Rc};

use image::RgbImage;
use libmemetendo::{
    bios,
    cart::{self, Cartridge},
    gba::Gba,
    util::{self, video::FrameBuffer},
    video::{self, HBLANK_DOT, VBLANK_DOT},
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
    pub screen: VideoCallback,
}

impl Runner {
    pub fn new(test_rom: cart::Rom) -> Self {
        let mut gba = Gba::new(BIOS_ROM.with(bios::Rom::clone), Cartridge::from(test_rom));
        gba.reset(true);

        Self {
            gba,
            screen: VideoCallback::new(),
        }
    }

    pub fn step(&mut self) {
        self.gba
            .step(&mut self.screen, &mut util::audio::NullCallback);
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

pub struct VideoCallback {
    pub image: RgbImage,
    new_frame: bool,
    buf: FrameBuffer,
}

impl Default for VideoCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoCallback {
    fn new() -> Self {
        Self {
            image: RgbImage::new(HBLANK_DOT.into(), VBLANK_DOT.into()),
            new_frame: false,
            buf: FrameBuffer::default(),
        }
    }
}

impl video::Callback for VideoCallback {
    fn put_dot(&mut self, x: u8, y: u8, dot: video::Dot) {
        self.buf.put_dot(x, y, dot);
    }

    fn end_frame(&mut self, green_swap: bool) {
        self.new_frame = true;
        if green_swap {
            self.buf.green_swap();
        }

        self.image
            .as_flat_samples_mut()
            .as_mut_slice()
            .copy_from_slice(&self.buf.0);
    }

    fn is_frame_skipping(&self) -> bool {
        false
    }
}
