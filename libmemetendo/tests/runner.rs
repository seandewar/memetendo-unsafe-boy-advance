use std::{mem::replace, path::Path};

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

pub enum TaskStatus {
    Pass,
    Fail,
    NotDone,
}

pub trait Task {
    fn check_task(&mut self, gba: &Gba, screen: &Screen) -> TaskStatus;

    fn on_success(&mut self, _gba: &mut Gba, _screen: &mut Screen) {}

    fn on_fail(&mut self, _gba: &mut Gba, _screen: &mut Screen) {
        panic!("task failed!");
    }

    fn on_timeout(&mut self, _gba: &mut Gba, _screen: &mut Screen) {
        panic!("task timed out!");
    }
}

pub struct Runner {
    test_rom: Rom,
}

impl Runner {
    pub fn new(test_path: impl AsRef<Path>) -> Self {
        let test_rom = Rom::from_file(test_path)
            .expect("failed to read test ROM; did you fetch the submodules?");

        Self { test_rom }
    }

    pub fn run(&self, max_frames: u32, task: &mut dyn Task) {
        let bios = Bios::new(&BIOS_ROM).expect("bad BIOS ROM");
        let cart = Cartridge::new(&self.test_rom).expect("bad test ROM");

        let mut screen = Screen::new();
        let mut gba = Gba::new(bios, cart);
        gba.reset(true);

        for frame in 0..max_frames {
            while !replace(&mut screen.is_new_frame, false) {
                gba.step(&mut screen);
            }

            match task.check_task(&gba, &screen) {
                TaskStatus::NotDone => continue,
                TaskStatus::Pass => {
                    println!("task passed (frame #{frame})");
                    task.on_success(&mut gba, &mut screen);
                    return;
                }
                TaskStatus::Fail => {
                    println!("task failed (frame #{frame})");
                    task.on_fail(&mut gba, &mut screen);
                    return;
                }
            }
        }

        println!("task timed out after {max_frames} frames");
        task.on_timeout(&mut gba, &mut screen);
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
