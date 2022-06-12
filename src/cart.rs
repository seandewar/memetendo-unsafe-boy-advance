use std::{fs, io, path::Path};

#[derive(Debug)]
pub struct Cartridge {
    rom: Vec<u8>,
    pub sram: Box<[u8]>,
}

impl Cartridge {
    pub fn from_file(path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self {
            rom: fs::read(path)?,
            sram: vec![0; 0x1_0000].into_boxed_slice(),
        })
    }

    pub fn rom(&self) -> &[u8] {
        &self.rom
    }
}

pub struct Bios(Box<[u8]>);

impl Bios {
    pub fn from_file(path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self(fs::read(path)?.into_boxed_slice()))
    }

    pub fn rom(&self) -> &[u8] {
        &self.0
    }
}
