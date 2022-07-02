//! Test runner for DenSinH/FuzzARM

mod runner;
mod util;

use std::{borrow::Cow, path::Path};

use image::RgbImage;
use libmemetendo::bus::Bus;
use once_cell::sync::Lazy;
use runner::Runner;
use util::{read_image, read_test_rom};

static PASS_SCREEN: Lazy<RgbImage> = Lazy::new(|| read_image("tests/fuzz_arm/ok.png"));

fn run_test(path: impl AsRef<Path>) {
    let rom = read_test_rom(path);
    let mut runner = Runner::new(&rom);
    for _ in 0..1000 {
        runner.step_frame();
        if runner.gba.ewram[..4].iter().any(|&b| b != 0) {
            failed(runner);
        } else if runner.screen.image == *PASS_SCREEN {
            return;
        }
    }

    panic!("timed out!");
}

fn failed(mut runner: Runner) -> ! {
    runner.step_frames(5); // Wait a bit for the results dump

    let mut ewram = runner.gba.ewram.as_ref();
    let state = match &ewram[..4] {
        b"AAAA" => Cow::Borrowed("Arm"),
        b"TTTT" => Cow::Borrowed("Thumb"),
        state => Cow::Owned(format!("Unknown ({})", String::from_utf8_lossy(state))),
    };
    let instr = String::from_utf8_lossy(&ewram[4..16]);

    let in_r0 = ewram.read_word(16);
    let in_r1 = ewram.read_word(20);
    let in_r2 = ewram.read_word(24);
    let in_cpsr = ewram.read_word(28);

    let out_r3 = ewram.read_word(32);
    let out_r4 = ewram.read_word(36);
    let out_cpsr = ewram.read_word(44);

    let expected_r3 = ewram.read_word(48);
    let expected_r4 = ewram.read_word(52);
    let expected_cpsr = ewram.read_word(60);

    panic!(
        "FuzzARM test failed!\n\
         State:         {state}\n\
         Instr:         {instr}\n\n\
         Input r0:      {in_r0:#010x}\n\
         Input r1:      {in_r1:#010x}\n\
         Input r2:      {in_r2:#010x}\n\
         Input cpsr:    {in_cpsr:#010x}\n\n\
         Result r3:     {out_r3:#010x}\n\
         Result r4:     {out_r4:#010x}\n\
         Result cpsr:   {out_cpsr:#010x}\n\n\
         Expected r3:   {expected_r3:#010x}\n\
         Expected r4:   {expected_r4:#010x}\n\
         Expected cpsr: {expected_cpsr:#010x}"
    );
}

// These tests are slow, especially on debug builds, so they are ignored by default.
#[ignore]
#[test]
fn arm_any() {
    run_test("tests/fuzz_arm/FuzzARM/ARM_Any.gba");
}

#[ignore]
#[test]
fn arm_data_processing() {
    run_test("tests/fuzz_arm/FuzzARM/ARM_DataProcessing.gba");
}

#[ignore]
#[test]
fn thumb_any() {
    run_test("tests/fuzz_arm/FuzzARM/THUMB_Any.gba");
}

#[ignore]
#[test]
fn thumb_data_processing() {
    run_test("tests/fuzz_arm/FuzzARM/THUMB_DataProcessing.gba");
}
