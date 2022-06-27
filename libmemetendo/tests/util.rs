use image::RgbImage;

pub fn read_image(path: impl AsRef<str>) -> RgbImage {
    image::io::Reader::open(path.as_ref())
        .expect("failed to open image file")
        .decode()
        .expect("failed to decode image")
        .into_rgb8()
}
