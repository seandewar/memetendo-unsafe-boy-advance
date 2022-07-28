use std::mem::take;

use intbits::Bits;

use crate::irq::{Interrupt, Irq};

#[derive(Debug, Default)]
pub struct Register {
    accum: u32,
    pub initial: u16,
    counter: u16,
    frequency: u8,
    pub cascade: bool,
    pub irq_enabled: bool,
    pub start: bool,
    bits: u16,
}

impl Register {
    /// # Panics
    ///
    /// Panics if the given byte index is out of bounds (> 3).
    pub fn set_byte(&mut self, idx: usize, value: u8) {
        match idx {
            0 => self.initial.set_bits(..8, value.into()),
            1 => self.initial.set_bits(8.., value.into()),
            2 => {
                self.frequency = value.bits(..2);
                self.cascade = value.bit(2);
                self.irq_enabled = value.bit(6);

                let old_start = self.start;
                self.start = value.bit(7);
                if !old_start && self.start {
                    self.counter = self.initial;
                }

                self.bits.set_bits(..8, value.into());
            }
            3 => self.bits.set_bits(8.., value.into()),
            _ => panic!("byte index out of bounds"),
        }
    }

    /// # Panics
    ///
    /// Panics if the given byte index is out of bounds (> 3).
    #[must_use]
    pub fn byte(&self, idx: usize) -> u8 {
        #[allow(clippy::cast_possible_truncation)]
        match idx {
            0 => self.counter as u8,
            1 => self.counter.bits(8..) as u8,
            2 => {
                let mut bits = self.bits as u8;
                bits.set_bits(..2, self.frequency);
                bits.set_bit(2, self.cascade);
                bits.set_bit(6, self.irq_enabled);
                bits.set_bit(7, self.start);

                bits
            }
            3 => self.bits.bits(8..) as u8,
            _ => panic!("byte index out of bounds"),
        }
    }
}

#[derive(Debug, Default)]
pub struct Timers(pub [Register; 4]);

impl Timers {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn step(&mut self, irq: &mut Irq, cycles: u32) {
        let mut prev_counter_overflowed = false;
        for (i, timer) in self.0.iter_mut().enumerate() {
            {
                let prev_counter_overflowed = take(&mut prev_counter_overflowed);
                if !timer.start || (timer.cascade && !prev_counter_overflowed) {
                    continue;
                }
            }

            if !timer.cascade {
                const MAX_DIV: u32 = 1024;

                let div = match timer.frequency {
                    0 => 1,
                    1 => 64,
                    2 => 256,
                    3 => MAX_DIV,
                    _ => unreachable!(),
                };
                timer.accum += cycles * MAX_DIV / div;
                if timer.accum < MAX_DIV {
                    continue;
                }

                timer.accum %= MAX_DIV;
            }

            let (new_counter, overflowed) = timer.counter.overflowing_add(1);
            if overflowed {
                prev_counter_overflowed = true;
                timer.counter = timer.initial;

                if timer.irq_enabled {
                    irq.request(match i {
                        0 => Interrupt::Timer0,
                        1 => Interrupt::Timer1,
                        2 => Interrupt::Timer2,
                        3 => Interrupt::Timer3,
                        _ => unreachable!(),
                    });
                }
            } else {
                timer.counter = new_counter;
            }
        }
    }
}
