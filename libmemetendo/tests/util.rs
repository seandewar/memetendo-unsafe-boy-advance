use std::{fs, path::Path, rc::Rc};

use image::RgbImage;
use libmemetendo::cart;

pub fn read_image(path: impl AsRef<Path>) -> RgbImage {
    image::io::Reader::open(path)
        .expect("failed to open image file")
        .decode()
        .expect("failed to decode image")
        .into_rgb8()
}

pub fn read_cart_rom(path: impl AsRef<Path>) -> cart::Rom {
    cart::Rom::new(Rc::from(
        fs::read(path).expect("failed to read test ROM; did you fetch the submodules?"),
    ))
    .expect("bad ROM size")
}
