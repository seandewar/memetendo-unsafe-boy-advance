//! Test runner for jsmolka/gba-tests

mod runner;
mod util;

use std::path::Path;

use image::{self, RgbImage};
use libmemetendo::gba::Gba;
use once_cell::sync::Lazy;
use runner::{Runner, Screen, TaskStatus};
use util::read_image;

struct Task<'a> {
    pass_screen: &'a RgbImage,
}

impl runner::Task for Task<'_> {
    fn check_task(&mut self, _gba: &Gba, screen: &Screen) -> TaskStatus {
        if screen.image == *self.pass_screen {
            TaskStatus::Pass
        } else {
            TaskStatus::NotDone
        }
    }
}

fn run_test(test_path: impl AsRef<Path>, pass_screen: &RgbImage) {
    Runner::new(test_path).run(4, &mut Task { pass_screen });
}

static PASS_SCREEN: Lazy<RgbImage> = Lazy::new(|| read_image("tests/jsmolka/ok.png"));

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
