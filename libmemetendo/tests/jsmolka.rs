//! Test runner for jsmolka/gba-tests

mod runner;
mod util;

use std::path::Path;

use image::RgbImage;
use once_cell::sync::Lazy;
use runner::Runner;
use util::{read_image, read_test_rom};

static PASS_SCREEN: Lazy<RgbImage> = Lazy::new(|| read_image("tests/jsmolka/ok.png"));

fn run_test(path: impl AsRef<Path>, pass_screen: &RgbImage) {
    let rom = read_test_rom(path);
    let mut runner = Runner::new(&rom);
    for _ in 0..3 {
        runner.step_frame();
        if runner.screen.image == *pass_screen {
            return;
        }
    }

    panic!("test failed");
}

// These tests use undefined behaviour that we do not handle right now.
#[ignore]
#[test]
fn arm() {
    run_test("tests/jsmolka/gba-tests/arm/arm.gba", &PASS_SCREEN);
}

#[test]
fn thumb() {
    run_test("tests/jsmolka/gba-tests/thumb/thumb.gba", &PASS_SCREEN);
}

#[test]
fn memory() {
    run_test("tests/jsmolka/gba-tests/memory/memory.gba", &PASS_SCREEN);
}

#[test]
fn bios() {
    run_test("tests/jsmolka/gba-tests/bios/bios.gba", &PASS_SCREEN);
}

#[test]
fn ppu_stripes() {
    let pass_screen = read_image("tests/jsmolka/ppu_stripes_ok.png");
    run_test("tests/jsmolka/gba-tests/ppu/stripes.gba", &pass_screen);
}

#[test]
fn ppu_shades() {
    let pass_screen = read_image("tests/jsmolka/ppu_shades_ok.png");
    run_test("tests/jsmolka/gba-tests/ppu/shades.gba", &pass_screen);
}
