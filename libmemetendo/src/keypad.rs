use std::ops::{Index, IndexMut};

use intbits::Bits;

use crate::{
    bus::Bus,
    irq::{Interrupt, Irq},
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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

impl Key {
    fn index(self) -> usize {
        self as _
    }
}

#[derive(Default, Copy, Clone, Eq, PartialEq, Debug)]
pub struct KeyStates([bool; 10]);

impl Index<Key> for KeyStates {
    type Output = bool;

    fn index(&self, key: Key) -> &Self::Output {
        &self.0[key.index()]
    }
}

impl IndexMut<Key> for KeyStates {
    fn index_mut(&mut self, key: Key) -> &mut Self::Output {
        &mut self.0[key.index()]
    }
}

#[derive(Default, Copy, Clone, Debug)]
pub struct Keypad {
    pub pressed: KeyStates,
    keycnt: InterruptControl,
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

        let mut iter = self
            .keycnt
            .irq_keys
            .0
            .into_iter()
            .zip(self.pressed.0.into_iter());

        let do_irq = if self.keycnt.irq_all_pressed {
            self.keycnt.irq_keys.0.into_iter().any(|irq| irq)
                && iter.all(|(irq, pressed)| !irq || pressed)
        } else {
            iter.any(|(irq, pressed)| irq && pressed)
        };
        if do_irq {
            irq.request(Interrupt::Keypad);
        }
    }
}

impl Bus for Keypad {
    fn read_byte(&mut self, addr: u32) -> u8 {
        match addr {
            // KEYINPUT
            0x130 => {
                let mut bits = 0;
                bits.set_bit(0, !self.pressed[Key::A]);
                bits.set_bit(1, !self.pressed[Key::B]);
                bits.set_bit(2, !self.pressed[Key::Select]);
                bits.set_bit(3, !self.pressed[Key::Start]);
                bits.set_bit(4, !self.pressed[Key::Right]);
                bits.set_bit(5, !self.pressed[Key::Left]);
                bits.set_bit(6, !self.pressed[Key::Up]);
                bits.set_bit(7, !self.pressed[Key::Down]);

                bits
            }
            0x131 => {
                let mut bits = 0xff;
                bits.set_bit(0, !self.pressed[Key::R]);
                bits.set_bit(1, !self.pressed[Key::L]);

                bits
            }
            // KEYCNT
            0x132 => self.keycnt.lo_bits(),
            0x133 => self.keycnt.hi_bits(),
            _ => panic!("IO register address OOB"),
        }
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        match addr {
            // KEYCNT
            0x132 => self.keycnt.set_lo_bits(value),
            0x133 => self.keycnt.set_hi_bits(value),
            0x130 | 0x131 => {}
            _ => panic!("IO register address OOB"),
        }
    }
}

#[derive(Default, Copy, Clone, Debug)]
struct InterruptControl {
    pub irq_keys: KeyStates,
    pub irq_enabled: bool,
    pub irq_all_pressed: bool,
}

impl InterruptControl {
    #[must_use]
    pub fn lo_bits(&self) -> u8 {
        let mut bits = 0;
        bits.set_bit(0, self.irq_keys[Key::A]);
        bits.set_bit(1, self.irq_keys[Key::B]);
        bits.set_bit(2, self.irq_keys[Key::Select]);
        bits.set_bit(3, self.irq_keys[Key::Start]);
        bits.set_bit(4, self.irq_keys[Key::Right]);
        bits.set_bit(5, self.irq_keys[Key::Left]);
        bits.set_bit(6, self.irq_keys[Key::Up]);
        bits.set_bit(7, self.irq_keys[Key::Down]);

        bits
    }

    #[must_use]
    pub fn hi_bits(&self) -> u8 {
        let mut bits = 0xff;
        bits.set_bit(0, self.irq_keys[Key::R]);
        bits.set_bit(1, self.irq_keys[Key::L]);

        bits.set_bit(6, self.irq_enabled);
        bits.set_bit(7, self.irq_all_pressed);

        bits
    }

    pub fn set_lo_bits(&mut self, bits: u8) {
        self.irq_keys[Key::A] = bits.bit(0);
        self.irq_keys[Key::B] = bits.bit(1);
        self.irq_keys[Key::Select] = bits.bit(2);
        self.irq_keys[Key::Start] = bits.bit(3);
        self.irq_keys[Key::Right] = bits.bit(4);
        self.irq_keys[Key::Left] = bits.bit(5);
        self.irq_keys[Key::Up] = bits.bit(6);
        self.irq_keys[Key::Down] = bits.bit(7);
    }

    pub fn set_hi_bits(&mut self, bits: u8) {
        self.irq_keys[Key::R] = bits.bit(0);
        self.irq_keys[Key::L] = bits.bit(1);

        self.irq_enabled = bits.bit(6);
        self.irq_all_pressed = bits.bit(7);
    }
}
