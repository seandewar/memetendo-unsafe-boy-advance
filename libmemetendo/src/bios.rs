use std::rc::Rc;

use crate::{bus::Bus, InvalidRomSize};

#[derive(Clone)]
pub struct Rom(Rc<[u8]>);

impl TryFrom<Rc<[u8]>> for Rom {
    type Error = InvalidRomSize;

    /// # Errors
    /// Returns an error if the size of the BIOS ROM image is not 16KiB.
    fn try_from(buf: Rc<[u8]>) -> Result<Self, Self::Error> {
        if buf.len() != 0x4000 {
            return Err(InvalidRomSize);
        }

        Ok(Self(buf))
    }
}

impl Rom {
    /// See `Self::try_from(Rc<[u8]>)`
    #[allow(clippy::missing_errors_doc)]
    pub fn new(buf: Rc<[u8]>) -> Result<Self, InvalidRomSize> {
        Self::try_from(buf)
    }
}

#[derive(Clone)]
pub struct Bios {
    rom: Rom,
    readable: bool,
    prefetch_addr: u32,
}

impl Bios {
    #[must_use]
    pub fn new(rom: Rom) -> Self {
        Self {
            rom,
            readable: false,
            prefetch_addr: 0,
        }
    }

    pub fn reset(&mut self) {
        self.readable = false;
        self.prefetch_addr = 0;
    }

    pub fn update_protection(&mut self, prefetch_addr: u32) {
        self.readable = prefetch_addr < 0x4000;
        if self.readable {
            self.prefetch_addr = prefetch_addr & !0b11;
        }
    }
}

impl Bus for Bios {
    fn read_byte(&mut self, addr: u32) -> u8 {
        self.rom.0.as_ref().read_byte(if self.readable {
            addr
        } else {
            self.prefetch_addr | (addr & 0b11)
        })
    }
}
