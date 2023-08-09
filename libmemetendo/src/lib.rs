#![warn(clippy::pedantic)]

use core::fmt;
use std::{
    error::Error,
    fmt::{Display, Formatter},
};

pub mod arm7tdmi;
pub mod audio;
pub mod bios;
pub mod bus;
pub mod cart;
pub mod dma;
pub mod gba;
pub mod irq;
pub mod keypad;
pub mod timer;
pub mod util;
pub mod video;

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub struct InvalidRomSize;

impl Display for InvalidRomSize {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Invalid ROM size")
    }
}

impl Error for InvalidRomSize {}
