use std::path::Path;

use image::RgbImage;
use libmemetendo::cart::Rom;

pub fn read_image(path: impl AsRef<Path>) -> RgbImage {
    image::io::Reader::open(path)
        .expect("failed to open image file")
        .decode()
        .expect("failed to decode image")
        .into_rgb8()
}

pub fn read_test_rom(path: impl AsRef<Path>) -> Rom<'static> {
    Rom::new(Box::leak(
        std::fs::read(path)
            .expect("failed to read test ROM; did you fetch the submodules?")
            .into_boxed_slice(),
    ))
    .expect("bad ROM size")
}
