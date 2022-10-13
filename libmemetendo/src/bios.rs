use crate::{bus::Bus, InvalidRomSize};

#[derive(Clone)]
pub struct Bios<'a> {
    rom: &'a [u8],
    readable: bool,
    prefetch_addr: u32,
}

impl<'a> TryFrom<&'a [u8]> for Bios<'a> {
    type Error = InvalidRomSize;

    /// # Errors
    ///
    /// Returns an error if the size of the BIOS ROM image is not 16KiB.
    fn try_from(rom: &'a [u8]) -> Result<Self, Self::Error> {
        if rom.len() != 0x4000 {
            return Err(InvalidRomSize);
        }

        Ok(Self {
            rom,
            readable: false,
            prefetch_addr: 0,
        })
    }
}

impl<'a> TryFrom<&'a mut [u8]> for Bios<'a> {
    type Error = InvalidRomSize;

    /// See `Self::try_from(&[u8])`.
    fn try_from(rom: &'a mut [u8]) -> Result<Self, Self::Error> {
        Self::try_from(&*rom)
    }
}

impl<'a> Bios<'a> {
    /// See `Self::try_from(&[u8])`.
    #[allow(clippy::missing_errors_doc)]
    pub fn new(rom: &'a [u8]) -> Result<Self, InvalidRomSize> {
        Self::try_from(rom)
    }

    pub fn reset(&mut self) {
        self.readable = false;
        self.prefetch_addr = 0;
    }

    pub fn update_protection(&mut self, prefetch_addr: Option<u32>) {
        self.readable = prefetch_addr.is_some();
        if let Some(addr) = prefetch_addr {
            self.prefetch_addr = addr & !0b11;
        }
    }
}

impl Bus for Bios<'_> {
    fn read_byte(&mut self, addr: u32) -> u8 {
        self.rom.read_byte(if self.readable {
            addr
        } else {
            self.prefetch_addr | (addr & 0b11)
        })
    }
}
