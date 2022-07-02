//! Test runner for destoer/armwrestler-gba-fixed

mod runner;
mod util;

use std::{ffi::OsStr, fs::read_dir, path::Path};

use libmemetendo::{keypad::Key, rom::Rom};
use once_cell::sync::Lazy;
use runner::Runner;
use util::{read_image, read_test_rom};

static TEST_ROM: Lazy<Rom> = Lazy::new(|| {
    read_test_rom("tests/armwrestler_gba/armwrestler-gba-fixed/armwrestler-gba-fixed.gba")
});

fn run_test(menu_entry_idx: u32, pass_screens_dir: impl AsRef<Path>) {
    let mut runner = Runner::new(&TEST_ROM);
    runner.step_frames(5); // Wait for startup
    for _ in 0..menu_entry_idx {
        runner.gba.keypad.pressed[Key::Down] = true;
        runner.step_frames(5);
        runner.gba.keypad.pressed[Key::Down] = false;
        runner.step_frames(5);
    }

    let mut pass_screens = read_dir(pass_screens_dir)
        .unwrap()
        .into_iter()
        .map(|entry| entry.unwrap().path())
        .filter_map(|path| {
            path.file_stem()
                .and_then(OsStr::to_str)
                .and_then(|s| s.parse::<u32>().ok())
                .map(|i| (i, read_image(path)))
        })
        .collect::<Vec<_>>();
    pass_screens.sort_unstable_by_key(|&(i, _)| i);

    for (i, screen) in pass_screens {
        runner.gba.keypad.pressed[Key::Start] = true;
        runner.step_frames(5);
        runner.gba.keypad.pressed[Key::Start] = false;
        runner.step_frames(10);

        // Don't use assert_eq! here; it'll pretty-print all of the bytes, which isn't useful.
        assert!(runner.screen.image == screen, "screen {i} did not match!");
    }
}

#[cfg_attr(debug_assertions, ignore)]
#[test]
fn arm_alu() {
    run_test(0, "tests/armwrestler_gba/arm_alu_ok");
}

#[cfg_attr(debug_assertions, ignore)]
#[test]
fn arm_ldr_str() {
    run_test(1, "tests/armwrestler_gba/arm_ldr_str_ok");
}

#[cfg_attr(debug_assertions, ignore)]
#[test]
fn arm_ldm_stm() {
    run_test(2, "tests/armwrestler_gba/arm_ldm_stm_ok");
}

#[cfg_attr(debug_assertions, ignore)]
#[test]
fn thumb_alu() {
    run_test(3, "tests/armwrestler_gba/thumb_alu_ok");
}

#[cfg_attr(debug_assertions, ignore)]
#[test]
fn thumb_ldr_str() {
    run_test(4, "tests/armwrestler_gba/thumb_ldr_str_ok");
}

#[cfg_attr(debug_assertions, ignore)]
#[test]
fn thumb_ldm_stm() {
    run_test(5, "tests/armwrestler_gba/thumb_ldm_stm_ok");
}
