#![warn(clippy::pedantic)]

mod arm7tdmi;
mod bus;
mod cart;
mod gba;
mod util;

use std::path::Path;

use cart::Cartridge;
use clap::{arg, command};
use gba::Gba;

fn main() {
    let matches = command!()
        .arg(arg!(<file> "ROM file to execute").allow_invalid_utf8(true))
        .get_matches();

    let rom_file = Path::new(matches.value_of_os("file").unwrap());
    let cart = Cartridge::from_file(rom_file).expect("failed to read cart");

    let mut gba = Gba::new(&cart);
    gba.reset();

    for _ in 0..10_000_000 {
        gba.step();
    }

    gba.write_fuzz_result("result.bin")
        .expect("failed to write result");
}
