use crate::{bus::Bus, InvalidRomSize};

use self::{eeprom::Eeprom, flash::Flash};

mod eeprom;
mod flash;

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub enum BackupType {
    #[default]
    None,
    EepromUnknownSize,
    Eeprom512B,
    Eeprom8KiB,
    Sram32KiB,
    Flash64KiB,
    Flash128KiB,
}

#[derive(Default, Copy, Clone)]
pub struct Rom<'a>(&'a [u8]);

impl<'a> TryFrom<&'a [u8]> for Rom<'a> {
    type Error = InvalidRomSize;

    /// # Errors
    /// Returns an error if the size of the cartridge ROM image exceeds 32MiB.
    fn try_from(buf: &'a [u8]) -> Result<Self, Self::Error> {
        if buf.len() > 0x200_0000 {
            return Err(InvalidRomSize);
        }

        Ok(Self(buf))
    }
}

impl<'a> TryFrom<&'a mut [u8]> for Rom<'a> {
    type Error = InvalidRomSize;

    /// See `Self::try_from(&[u8])`
    fn try_from(buf: &'a mut [u8]) -> Result<Self, Self::Error> {
        Self::try_from(&*buf)
    }
}

impl<'a> Rom<'a> {
    /// See `Self::try_from(&[u8])`
    #[allow(clippy::missing_errors_doc)]
    pub fn new(buf: &'a [u8]) -> Result<Self, InvalidRomSize> {
        Self::try_from(buf)
    }

    #[must_use]
    pub fn parse_backup_type(&self) -> BackupType {
        // Search for valid IDs in the format "{id_prefix}_Vnnn".
        // They are word-aligned (4 bytes) and 0-padded.
        for i in (0..self.0.len()).step_by(4) {
            let has_id = |id_prefix: &[u8]| {
                let version_fmt = b"_Vnnn";
                let id_len = id_prefix.len() + version_fmt.len();
                let padding_len = if id_len % 4 > 0 { 4 - id_len % 4 } else { 0 };
                let slice = &self.0[i..];

                slice.len() >= id_len + padding_len
                    && slice.starts_with(id_prefix)
                    && slice[id_len..id_len + padding_len].iter().all(|&b| b == 0)
            };

            if has_id(b"EEPROM") {
                // Impossible to detect the EEPROM's size from inspecting the ROM.
                // Try and detect it at runtime.
                return BackupType::EepromUnknownSize;
            } else if has_id(b"SRAM") || has_id(b"SRAM_F") {
                return BackupType::Sram32KiB;
            } else if has_id(b"FLASH") || has_id(b"FLASH512") {
                return BackupType::Flash64KiB;
            } else if has_id(b"FLASH1M") {
                return BackupType::Flash128KiB;
            }
        }

        BackupType::None
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.0
    }
}

#[derive(Clone)]
pub struct Cartridge<'r> {
    rom: Rom<'r>,
    backup: Option<Backup>,
}

impl<'r> From<Rom<'r>> for Cartridge<'r> {
    fn from(rom: Rom<'r>) -> Self {
        Self::new(rom, rom.parse_backup_type())
    }
}

#[derive(Clone)]
enum Backup {
    EepromUnknownSize,
    Eeprom(Eeprom),
    Sram(Box<[u8]>),
    Flash(Flash),
}

impl<'r> Cartridge<'r> {
    #[must_use]
    pub fn new(rom: Rom<'r>, backup_type: BackupType) -> Self {
        Self {
            rom,
            backup: match backup_type {
                BackupType::None => None,
                BackupType::EepromUnknownSize => Some(Backup::EepromUnknownSize),
                BackupType::Eeprom512B => Some(Backup::Eeprom(Eeprom::new(false))),
                BackupType::Eeprom8KiB => Some(Backup::Eeprom(Eeprom::new(true))),
                BackupType::Sram32KiB => Some(Backup::Sram(vec![0xff; 32 * 1024].into())),
                BackupType::Flash64KiB => Some(Backup::Flash(Flash::new(false))),
                BackupType::Flash128KiB => Some(Backup::Flash(Flash::new(true))),
            },
        }
    }

    #[must_use]
    pub fn rom(&self) -> &Rom {
        &self.rom
    }

    pub(crate) fn is_eeprom_offset(&self, offset: u32) -> bool {
        matches!(
            self.backup,
            Some(Backup::Eeprom(_) | Backup::EepromUnknownSize)
        ) && (offset & 0x1ff_ff00 == 0x1ff_ff00
            || (self.rom.bytes().len() <= 16 * 1024 * 1024 && offset >= 0x500_0000))
    }

    fn access_eeprom(&mut self, addr: u32) -> Option<&mut Eeprom> {
        self.is_eeprom_offset(addr).then(|| {
            if let Some(Backup::EepromUnknownSize) = self.backup {
                // No idea what the size is, but we need something to access... just assume 512B.
                println!("could not guess EEPROM size; falling back to 512B!");
                self.backup = Some(Backup::Eeprom(Eeprom::new(false)));
            }

            if let Some(Backup::Eeprom(eeprom)) = self.backup.as_mut() {
                eeprom
            } else {
                unreachable!()
            }
        })
    }

    pub(crate) fn notify_eeprom_dma(&mut self, blocks: u32) {
        if !matches!(self.backup, Some(Backup::EepromUnknownSize)) {
            return;
        }

        // Guess the EEPROM's size from the number of DMA blocks requested.
        match blocks {
            // 6-bit addr read or write: 512B.
            9 | 73 => {
                println!("guessing 512B EEPROM size");
                self.backup = Some(Backup::Eeprom(Eeprom::new(false)));
            }
            // 14-bit addr read or write: 8KiB.
            17 | 81 => {
                println!("guessing 8KiB EEPROM size");
                self.backup = Some(Backup::Eeprom(Eeprom::new(true)));
            }
            _ => {}
        }
    }
}

impl Bus for Cartridge<'_> {
    fn read_byte(&mut self, addr: u32) -> u8 {
        match addr {
            // TODO: WAITCNT with wait states 0, 1 and 2
            0x000_0000..=0x1ff_ffff | 0x200_0000..=0x3ff_ffff | 0x400_0000..=0x5ff_ffff => {
                if let Some(eeprom) = self.access_eeprom(addr) {
                    eeprom.read_byte(addr)
                } else {
                    self.rom
                        .bytes()
                        .get(addr as usize & 0x1ff_ffff)
                        .copied()
                        .unwrap_or(0)
                }
            }
            0x600_0000..=0x7ff_ffff => match self.backup.as_mut() {
                Some(Backup::Sram(sram)) => sram.read_byte(addr & 0x7fff),
                Some(Backup::Flash(flash)) => flash.read_byte(addr & 0xffff),
                _ => 0xff,
            },
            _ => panic!("cartridge address OOB"),
        }
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        match addr {
            // TODO: WAITCNT with wait states 0, 1 and 2
            0x000_0000..=0x1ff_ffff | 0x200_0000..=0x3ff_ffff | 0x400_0000..=0x5ff_ffff => {
                if let Some(eeprom) = self.access_eeprom(addr) {
                    eeprom.write_byte(addr, value);
                }
            }
            0x600_0000..=0x7ff_ffff => match self.backup.as_mut() {
                Some(Backup::Sram(sram)) => sram.write_byte(addr & 0x7fff, value),
                Some(Backup::Flash(flash)) => flash.write_byte(addr & 0xffff, value),
                _ => {}
            },
            _ => panic!("cartridge address OOB"),
        }
    }
}
