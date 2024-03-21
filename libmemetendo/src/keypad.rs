use intbits::Bits;
use strum_macros::EnumCount;

use crate::{
    bus::Bus,
    irq::{Interrupt, Irq},
};

#[derive(Debug, Copy, Clone, PartialEq, Eq, EnumCount)]
pub enum Key {
    A,
    B,
    Select,
    Start,
    Right,
    Left,
    Up,
    Down,
    R,
    L,
}

#[derive(Default, Copy, Clone, Debug)]
struct IrqControl {
    keys: u16,
    enabled: bool,
    all_pressed: bool,
}

#[derive(Default, Copy, Clone, Debug)]
pub struct Keypad {
    pressed: u16,
    keycnt: IrqControl,
}

impl Keypad {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn step(&mut self, irq: &mut Irq) {
        if !self.keycnt.enabled {
            return;
        }

        let do_irq = if self.keycnt.all_pressed {
            self.pressed != 0 && self.keycnt.keys & self.pressed == self.pressed
        } else {
            self.keycnt.keys & self.pressed != 0
        };
        if do_irq {
            irq.request(Interrupt::Keypad);
        }
    }

    pub fn set_pressed(&mut self, key: Key, pressed: bool) {
        self.pressed.set_bit(key as usize, pressed);
    }
}

impl Bus for Keypad {
    fn read_byte(&mut self, addr: u32) -> u8 {
        match addr {
            // KEYINPUT
            0x130 => (!self.pressed).bits(..8).try_into().unwrap(),
            0x131 => (!self.pressed).bits(8..).try_into().unwrap(),
            // KEYCNT
            0x132 => self.keycnt.keys.bits(..8).try_into().unwrap(),
            0x133 => u8::try_from(self.keycnt.keys.bits(8..))
                .unwrap()
                .with_bit(6, self.keycnt.enabled)
                .with_bit(7, self.keycnt.all_pressed),
            _ => panic!("IO register address OOB"),
        }
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        match addr {
            // KEYCNT
            0x132 => self.keycnt.keys.set_bits(..8, value.into()),
            0x133 => self.keycnt.keys.set_bits(8.., value.into()),
            0x130 | 0x131 => {}
            _ => panic!("IO register address OOB"),
        }
    }
}
