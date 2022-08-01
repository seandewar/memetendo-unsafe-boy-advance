use intbits::Bits;

use crate::{
    arm7tdmi::{Cpu, Exception},
    bus::Bus,
    gba::{HaltControl, State},
};

#[derive(Debug, Copy, Clone)]
pub enum Interrupt {
    VBlank,
    HBlank,
    VCount,
    Timer0,
    Timer1,
    Timer2,
    Timer3,
    Serial,
    Dma0,
    Dma1,
    Dma2,
    Dma3,
    Keypad,
    GamePak,
}

#[derive(Debug, Default)]
pub struct Irq {
    intme: u32,
    inte: u16,
    intf: u16,
}

impl Irq {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn step(&mut self, cpu: &mut Cpu, haltcnt: &mut HaltControl) {
        if (self.inte.bits(..14) & self.intf) == 0 {
            return;
        }

        haltcnt.0 = State::Running;
        if self.intme.bit(0) {
            cpu.raise_exception(Exception::Interrupt);
        }
    }

    pub fn request(&mut self, interrupt: Interrupt) {
        self.intf.set_bit(interrupt as usize, true);
    }
}

impl Bus for Irq {
    fn read_byte(&mut self, addr: u32) -> u8 {
        #[allow(clippy::cast_possible_truncation)]
        match addr {
            // IE
            0x200 => self.inte as u8,
            0x201 => self.inte.bits(8..) as u8,
            // IF
            0x202 => self.intf as u8,
            0x203 => self.intf.bits(8..) as u8,
            // IME
            0x208 => self.intme as u8,
            0x209 => self.intme.bits(8..16) as u8,
            0x20a => self.intme.bits(16..24) as u8,
            0x20b => self.intme.bits(24..) as u8,
            _ => panic!("IO register address OOB"),
        }
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        match addr {
            // IE
            0x200 => self.inte.set_bits(..8, value.into()),
            0x201 => self.inte.set_bits(8.., value.into()),
            // IF
            0x202 => self
                .intf
                .set_bits(..8, self.intf.bits(..8) & u16::from(!value)),
            0x203 => self
                .intf
                .set_bits(8.., self.intf.bits(8..) & u16::from(!value)),
            // IME
            0x208 => self.intme.set_bits(..8, value.into()),
            0x209 => self.intme.set_bits(8..16, value.into()),
            0x20a => self.intme.set_bits(16..24, value.into()),
            0x20b => self.intme.set_bits(24.., value.into()),
            _ => panic!("IO register address OOB"),
        }
    }
}
