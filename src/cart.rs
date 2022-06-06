use std::{fs, io, path::Path};

#[derive(Debug)]
pub struct Cartridge {
    rom: Vec<u8>,
}

impl Cartridge {
    pub fn from_file(path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self {
            rom: fs::read(path)?,
        })
    }

    pub fn rom(&self) -> &[u8] {
        &self.rom
    }
}
