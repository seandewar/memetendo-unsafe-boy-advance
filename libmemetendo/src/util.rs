#[macro_export]
macro_rules! arbitrary_sign_extend {
    ($result_type:ty, $x:expr, $bitcount:expr) => {{
        i64::from($x)
            .wrapping_shl(64 - $bitcount)
            .wrapping_shr(64 - $bitcount) as $result_type
    }};

    ($x:expr, $bitcount:expr) => {
        arbitrary_sign_extend!(_, $x, $bitcount)
    };
}

pub mod video {
    use crate::video::{Callback, Dot, HBLANK_DOT, VBLANK_DOT};

    #[derive(Clone, Debug)]
    pub struct FrameBuffer<const STRIDE: usize = 3>(pub Box<[u8]>);

    impl<const STRIDE: usize> Default for FrameBuffer<STRIDE> {
        fn default() -> Self {
            Self::new(0)
        }
    }

    impl<const STRIDE: usize> FrameBuffer<STRIDE> {
        /// # Panics
        ///
        /// Panics if `STRIDE` < 3, as this is an RGB buffer.
        #[must_use]
        pub fn new(fill: u8) -> Self {
            assert!(STRIDE >= 3);
            Self(vec![fill; STRIDE * HBLANK_DOT as usize * VBLANK_DOT as usize].into_boxed_slice())
        }

        pub fn put_dot(&mut self, x: u8, y: u8, dot: Dot) {
            let i = STRIDE * (usize::from(y) * usize::from(HBLANK_DOT) + usize::from(x));
            self.0[i] = dot.red() * 8;
            self.0[i + 1] = dot.green() * 8;
            self.0[i + 2] = dot.blue() * 8;
        }

        pub fn green_swap(&mut self) {
            for i in (0..self.0.len()).step_by(STRIDE * 2) {
                self.0.swap(i + 1, i + STRIDE + 1);
            }
        }
    }

    pub struct NullCallback;

    impl Callback for NullCallback {
        fn put_dot(&mut self, _: u8, _: u8, _: Dot) {}

        fn end_frame(&mut self, _: bool) {}

        fn is_frame_skipping(&self) -> bool {
            false
        }
    }
}

pub mod audio {
    use crate::audio::Callback;

    pub struct NullCallback;

    impl Callback for NullCallback {
        fn push_sample(&mut self, _: (i16, i16)) {}
    }
}
