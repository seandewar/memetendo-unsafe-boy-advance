#![warn(clippy::pedantic)]

pub mod arm7tdmi;
pub mod bus;
pub mod gba;
pub mod irq;
pub mod keypad;
pub mod rom;
pub mod video;

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
