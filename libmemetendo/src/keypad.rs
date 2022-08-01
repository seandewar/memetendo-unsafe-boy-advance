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
struct Control {
    irq_keys: u16,
    irq_enabled: bool,
    irq_all_pressed: bool,
}

#[derive(Default, Copy, Clone, Debug)]
pub struct Keypad {
    pressed: u16,
    keycnt: Control,
}

impl Keypad {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn step(&mut self, irq: &mut Irq) {
        if !self.keycnt.irq_enabled {
            return;
        }

        let do_irq = if self.keycnt.irq_all_pressed {
            self.pressed != 0 && self.keycnt.irq_keys & self.pressed == self.pressed
        } else {
            self.keycnt.irq_keys & self.pressed != 0
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
        #[allow(clippy::cast_possible_truncation)]
        match addr {
            // KEYINPUT
            0x130 => !self.pressed as u8,
            0x131 => !self.pressed.bits(8..) as u8,
            // KEYCNT
            0x132 => self.keycnt.irq_keys as u8,
            0x133 => {
                let mut bits = self.keycnt.irq_keys.bits(8..) as u8;
                bits.set_bit(6, self.keycnt.irq_enabled);
                bits.set_bit(7, self.keycnt.irq_all_pressed);

                bits
            }
            _ => panic!("IO register address OOB"),
        }
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        match addr {
            // KEYCNT
            0x132 => self.keycnt.irq_keys.set_bits(..8, value.into()),
            0x133 => self.keycnt.irq_keys.set_bits(8.., value.into()),
            0x130 | 0x131 => {}
            _ => panic!("IO register address OOB"),
        }
    }
}
