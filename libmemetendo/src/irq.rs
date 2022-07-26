use intbits::Bits;

use crate::{
    arm7tdmi::{Cpu, Exception},
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
    pub state: State,
    pub intme: u32,
    pub inte: u16,
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

    pub fn set_intf_lo_bits(&mut self, value: u8) {
        self.intf
            .set_bits(..8, self.intf.bits(..8) & u16::from(!value));
    }

    pub fn set_intf_hi_bits(&mut self, value: u8) {
        self.intf
            .set_bits(8.., self.intf.bits(8..) & u16::from(!value));
    }

    #[must_use]
    pub fn intf(&self) -> u16 {
        self.intf
    }
}
