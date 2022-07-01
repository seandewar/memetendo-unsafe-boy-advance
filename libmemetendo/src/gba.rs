use intbits::Bits;

use crate::{
    arm7tdmi::Cpu,
    bus,
    keypad::Keypad,
    rom::{Bios, Cartridge},
    video::{self, screen::Screen},
};

pub struct Gba<'b, 'c> {
    pub cpu: Cpu,
    pub iwram: Box<[u8]>,
    pub ewram: Box<[u8]>,
    pub video: video::Controller,
    pub keypad: Keypad,
    pub bios: Bios<'b>,
    pub cart: Cartridge<'c>,
    io_todo: Box<[u8]>,
}

impl<'b, 'c> Gba<'b, 'c> {
    #[must_use]
    pub fn new(bios: Bios<'b>, cart: Cartridge<'c>) -> Self {
        Self {
            cpu: Cpu::new(),
            iwram: vec![0; 0x8000].into_boxed_slice(),
            ewram: vec![0; 0x40000].into_boxed_slice(),
            video: video::Controller::new(),
            keypad: Keypad::new(),
            bios,
            cart,
            io_todo: vec![0; 0x801].into_boxed_slice(),
        }
    }

    pub fn reset(&mut self, skip_bios: bool) {
        self.bios.reset();
        self.cpu.reset(&mut bus!(self), skip_bios);

        if skip_bios {
            self.iwram[0x7e00..].fill(0);
            self.bios.update_protection(Some(0xdc + 8));
        }
    }

    pub fn step(&mut self, screen: &mut impl Screen) {
        self.keypad.step(&mut self.cpu);
        self.cpu.step(&mut bus!(self));
        self.video.step(screen, &mut self.cpu, 8);
    }
}

pub struct Bus<'a, 'b, 'c> {
    pub iwram: &'a mut [u8],
    pub ewram: &'a mut [u8],
    pub video: &'a mut video::Controller,
    pub keypad: &'a mut Keypad,
    pub bios: &'a mut Bios<'b>,
    pub cart: &'a mut Cartridge<'c>,
    io_todo: &'a mut [u8],
}

// A member fn would be nicer, but using &mut self over $gba unnecessarily mutably borrows the
// *whole* Gba struct.
#[macro_export]
macro_rules! bus {
    ($gba:ident) => {{
        $crate::gba::Bus {
            iwram: &mut $gba.iwram,
            ewram: &mut $gba.ewram,
            video: &mut $gba.video,
            keypad: &mut $gba.keypad,
            cart: &mut $gba.cart,
            bios: &mut $gba.bios,
            io_todo: &mut $gba.io_todo,
        }
    }};
}

impl Bus<'_, '_, '_> {
    fn read_io(&self, addr: u32) -> u8 {
        match addr & 0x3ff {
            // DISPCNT
            0x0 => self.video.dispcnt.lo_bits(),
            0x1 => self.video.dispcnt.hi_bits(),
            // Green swap (undocumented)
            #[allow(clippy::cast_possible_truncation)]
            0x2 => self.video.green_swap as u8,
            #[allow(clippy::cast_possible_truncation)]
            0x3 => self.video.green_swap.bits(8..) as u8,
            // DISPSTAT
            0x4 => self.video.dispstat_lo_bits(),
            0x5 => self.video.dispstat.vcount_target,
            // VCOUNT
            0x6 => self.video.vcount(),
            0x7 => 0,
            // BG0CNT
            0x8 => self.video.bgcnt[0].lo_bits(),
            0x9 => self.video.bgcnt[0].hi_bits(),
            // BG1CNT
            0xa => self.video.bgcnt[1].lo_bits(),
            0xb => self.video.bgcnt[1].hi_bits(),
            // BG2CNT
            0xc => self.video.bgcnt[2].lo_bits(),
            0xd => self.video.bgcnt[2].hi_bits(),
            // BG3CNT
            0xe => self.video.bgcnt[3].lo_bits(),
            0xf => self.video.bgcnt[3].hi_bits(),
            // KEYINPUT
            0x130 => self.keypad.keyinput_lo_bits(),
            0x131 => self.keypad.keyinput_hi_bits(),
            // KEYCNT
            0x132 => self.keypad.keycnt.lo_bits(),
            0x133 => self.keypad.keycnt.hi_bits(),
            // TODO
            addr @ 0..=0x800 => self.io_todo[addr as usize],
            _ => 0,
        }
    }

    fn write_io(&mut self, addr: u32, value: u8) {
        match addr & 0x3ff {
            // DISPCNT
            0x0 => self.video.dispcnt.set_lo_bits(value),
            0x1 => self.video.dispcnt.set_hi_bits(value),
            // Green swap (undocumented)
            0x2 => self.video.green_swap.set_bits(..8, value.into()),
            0x3 => self.video.green_swap.set_bits(8.., value.into()),
            // DISPSTAT
            0x4 => self.video.dispstat.set_lo_bits(value),
            0x5 => self.video.dispstat.vcount_target = value,
            // BG0CNT
            0x8 => self.video.bgcnt[0].set_lo_bits(value),
            0x9 => self.video.bgcnt[0].set_hi_bits(value),
            // BG1CNT
            0xa => self.video.bgcnt[1].set_lo_bits(value),
            0xb => self.video.bgcnt[1].set_hi_bits(value),
            // BG2CNT
            0xc => self.video.bgcnt[2].set_lo_bits(value),
            0xd => self.video.bgcnt[2].set_hi_bits(value),
            // BG3CNT
            0xe => self.video.bgcnt[3].set_lo_bits(value),
            0xf => self.video.bgcnt[3].set_hi_bits(value),
            // KEYCNT
            0x132 => self.keypad.keycnt.set_lo_bits(value),
            0x133 => self.keypad.keycnt.set_hi_bits(value),
            // TODO
            addr @ 0..=0x800 => self.io_todo[addr as usize] = value,
            _ => {}
        }
    }
}

impl bus::Bus for Bus<'_, '_, '_> {
    fn read_byte(&mut self, addr: u32) -> u8 {
        match addr {
            // BIOS
            0x0000_0000..=0x0000_3fff => self.bios.read_byte(addr),
            // External WRAM
            0x0200_0000..=0x02ff_ffff => self.ewram.as_ref().read_byte(addr & 0x3_ffff),
            // Internal WRAM
            0x0300_0000..=0x03ff_ffff => self.iwram.as_ref().read_byte(addr & 0x7fff),
            // I/O Registers
            0x0400_0000..=0x0400_03fe => self.read_io(addr),
            // Palette RAM
            0x0500_0000..=0x05ff_ffff => self.video.palette_ram.as_ref().read_byte(addr & 0x3ff),
            // VRAM
            0x0600_0000..=0x06ff_ffff => self.video.vram.as_ref().read_byte(addr & 0x1_7fff),
            // OAM
            0x0700_0000..=0x07ff_ffff => self.video.oam.as_ref().read_byte(addr & 0x3ff),
            // ROM Mirror; TODO: Wait states 0, 1 and 2
            0x0800_0000..=0x09ff_ffff | 0x0a00_0000..=0x0bff_ffff | 0x0c00_0000..=0x0dff_ffff => {
                self.cart.read_byte(addr & 0x1ff_ffff)
            }
            // SRAM
            0x0e00_0000..=0x0e00_ffff => self.cart.sram.as_ref().read_byte(addr & 0xffff),
            // Unused
            _ => 0xff,
        }
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        match addr {
            // External WRAM
            0x0200_0000..=0x02ff_ffff => self.ewram.as_mut().write_byte(addr & 0x3_ffff, value),
            // Internal WRAM
            0x0300_0000..=0x03ff_ffff => self.iwram.as_mut().write_byte(addr & 0x7fff, value),
            // I/O Registers
            0x0400_0000..=0x0400_03fe => self.write_io(addr, value),
            // Palette RAM
            0x0500_0000..=0x05ff_ffff => {
                // 8-bit writes act weird; write as a hword.
                self.video
                    .palette_ram
                    .as_mut()
                    .write_hword(addr & 0x3ff, u16::from_le_bytes([value, value]));
            }
            // VRAM
            0x0600_0000..=0x06ff_ffff => {
                // Like palette RAM, but only write a hword for BG data.
                if (addr as usize & 0x1_7fff) < self.video.dispcnt.obj_vram_offset() {
                    self.video
                        .vram
                        .as_mut()
                        .write_hword(addr & 0x1_7fff, u16::from_le_bytes([value, value]));
                }
            }
            // SRAM
            0x0e00_0000..=0x0e00_ffff => self.cart.sram.as_mut().write_byte(addr & 0xffff, value),
            // Read-only, Unused, Ignored 8-bit writes to OAM/VRAM
            _ => {}
        }
    }

    fn write_hword(&mut self, addr: u32, value: u16) {
        // Video memory has weird behaviour when writing 8-bit values, so we can't simply delegate
        // such writes to write_hword_as_bytes.
        match addr {
            // Palette RAM
            0x0500_0000..=0x05ff_ffff => {
                self.video
                    .palette_ram
                    .as_mut()
                    .write_hword(addr & 0x3ff, value);
            }
            // VRAM
            0x0600_0000..=0x06ff_ffff => {
                self.video.vram.as_mut().write_hword(addr & 0x1_7fff, value);
            }
            // OAM
            0x0700_0000..=0x07ff_ffff => self.video.oam.as_mut().write_hword(addr & 0x3ff, value),
            _ => bus::write_hword_as_bytes(self, addr, value),
        }
    }

    fn prefetch_instr(&mut self, addr: u32) {
        self.bios.update_protection((addr < 0x4000).then_some(addr));
    }
}
