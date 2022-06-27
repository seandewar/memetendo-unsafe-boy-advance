use std::{borrow::Cow, path::Path};

use libmemetendo::{
    bus::Bus,
    gba::Gba,
    rom::{Bios, Cartridge, Rom},
    video::NullScreen,
};

fn run_test(test_path: impl AsRef<Path>) {
    let test_rom =
        Rom::from_file(test_path).expect("failed to read test ROM; did you fetch the submodules?");
    let bios_rom = Rom::from_file("tests/bios.bin").expect(
        "failed to read BIOS ROM; place it in a \"bios.bin\" file within the tests directory",
    );
    Runner::new(&bios_rom, &test_rom).run(1_500_000);
}

#[test]
fn arm_any() {
    run_test("tests/FuzzARM/ARM_Any.gba");
}

#[test]
fn arm_data_processing() {
    run_test("tests/FuzzARM/ARM_DataProcessing.gba");
}

#[test]
fn thumb_any() {
    run_test("tests/FuzzARM/THUMB_Any.gba");
}

#[test]
fn thumb_data_processing() {
    run_test("tests/FuzzARM/THUMB_DataProcessing.gba");
}

struct Runner<'a>(Gba<'a, 'a>);

impl<'a> Runner<'a> {
    fn new(bios_rom: &'a Rom, test_rom: &'a Rom) -> Self {
        let bios = Bios::new(bios_rom).expect("bad BIOS ROM");
        let cart = Cartridge::new(test_rom).expect("bad test ROM");

        Self(Gba::new(bios, cart))
    }

    fn run(&mut self, max_steps: u32) {
        self.0.reset(true);
        for step in 0..max_steps {
            self.0.step(&mut NullScreen);

            if (step % 100 == 99 || step == max_steps - 1) && self.check_finished() {
                println!("FuzzARM test passed after {step} steps!");
                return;
            }
        }

        panic!("FuzzARM test timed out after {max_steps} steps!");
    }

    fn check_finished(&mut self) -> bool {
        const EXPECTED_VRAM: &[u8] = include_bytes!("fuzz_arm_expected_vram.bin");

        // Test dumps 64 bytes to EWRAM when it fails. Check if the state bytes were modified.
        if self.0.ewram[..4].iter().any(|&b| b != 0) {
            self.panic_failed();
        }

        // Test writes 1504 bytes to VRAM to display "End of testing" when it succeeds.
        self.0.video.vram[1503] == 1 && &self.0.video.vram[..EXPECTED_VRAM.len()] == EXPECTED_VRAM
    }

    fn panic_failed(&mut self) {
        // Wait a bit for the results to be dumped.
        for _ in 0..250_000 {
            self.0.step(&mut NullScreen);
        }

        let mut ewram = self.0.ewram.as_ref();
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
}
