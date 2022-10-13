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
    let mut runner = Runner::new(read_test_rom(path));
    for _ in 0..3 {
        runner.step_frame();
        if runner.screen.image == *pass_screen {
            return;
        }
    }

    panic!("test failed");
}

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
fn nes() {
    run_test("tests/jsmolka/gba-tests/nes/nes.gba", &PASS_SCREEN);
}

#[test]
fn ppu_stripes() {
    run_test(
        "tests/jsmolka/gba-tests/ppu/stripes.gba",
        &read_image("tests/jsmolka/ppu_stripes_ok.png"),
    );
}

#[test]
fn ppu_shades() {
    run_test(
        "tests/jsmolka/gba-tests/ppu/shades.gba",
        &read_image("tests/jsmolka/ppu_shades_ok.png"),
    );
}

#[test]
fn save_none() {
    run_test("tests/jsmolka/gba-tests/save/none.gba", &PASS_SCREEN);
}
