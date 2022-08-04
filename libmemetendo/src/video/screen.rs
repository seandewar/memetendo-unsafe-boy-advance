use super::{HBLANK_DOT, VBLANK_DOT};

pub const WIDTH: usize = HBLANK_DOT as _;
pub const HEIGHT: usize = VBLANK_DOT as _;

pub trait Screen {
    fn finished_frame(&mut self, frame: &FrameBuffer);
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl From<u32> for Rgb {
    fn from(rgb: u32) -> Self {
        Self::from_le_bytes(rgb.to_le_bytes()[..3].try_into().unwrap())
    }
}

impl Rgb {
    #[must_use]
    pub fn from_le_bytes(bytes: [u8; 3]) -> Self {
        Self {
            r: bytes[0],
            g: bytes[1],
            b: bytes[2],
        }
    }

    #[must_use]
    pub fn to_le_bytes(self) -> [u8; 3] {
        [self.r, self.g, self.b]
    }
}

#[derive(Clone)]
pub struct FrameBuffer(pub Box<[u8]>);

impl Default for FrameBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameBuffer {
    #[must_use]
    pub fn new() -> Self {
        Self(vec![0; WIDTH * HEIGHT * 3].into_boxed_slice())
    }

    // This shouldn't panic, as the slice should always have 3 elements.
    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn pixel(&self, x: usize, y: usize) -> Rgb {
        let i = Self::pixel_index(x, y);
        Rgb::from_le_bytes(self.0[i..i + 3].try_into().unwrap())
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, rgb: Rgb, green_swap: bool) {
        let i = Self::pixel_index(x, y);
        self.0[i..i + 3].copy_from_slice(&rgb.to_le_bytes());
        if green_swap && x % 2 == 1 {
            self.0.swap(i + 1, i - 2);
        }
    }

    fn pixel_index(x: usize, y: usize) -> usize {
        (y * WIDTH + x) * 3
    }
}
