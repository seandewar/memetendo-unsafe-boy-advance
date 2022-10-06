use std::mem::{replace, take};

use intbits::Bits;

use crate::{
    audio::Audio,
    bus::Bus,
    irq::{Interrupt, Irq},
};

#[derive(Debug, Default)]
struct Control {
    accum: u32,
    initial: u16,
    counter: u16,
    frequency: u8,
    cascade: bool,
    irq_enabled: bool,
    start: bool,
    cached_bits: u16,
}

#[derive(Debug, Default)]
pub struct Timers([Control; 4]);

impl Timers {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn step(&mut self, irq: &mut Irq, audio: &mut Audio, cycles: u8) {
        let mut prev_overflow_count = 0;
        for (i, timer) in self.0.iter_mut().enumerate() {
            let ticks = {
                let prev_overflow_count = take(&mut prev_overflow_count);
                if !timer.start || (timer.cascade && prev_overflow_count == 0) {
                    continue;
                }

                if timer.cascade {
                    prev_overflow_count
                } else {
                    const MAX_DIV: u32 = 1024;

                    let div = match timer.frequency {
                        0 => 1,
                        1 => 64,
                        2 => 256,
                        3 => MAX_DIV,
                        _ => unreachable!(),
                    };
                    timer.accum += u32::from(cycles) * MAX_DIV / div;
                    if timer.accum < MAX_DIV {
                        continue;
                    }

                    #[allow(clippy::cast_possible_truncation)]
                    let ticks = (timer.accum / MAX_DIV) as u16;
                    timer.accum %= MAX_DIV;

                    ticks
                }
            };

            let (new_counter, overflowed) = timer.counter.overflowing_add(ticks);
            timer.counter = if overflowed {
                let extra_ticks = u32::from(ticks - (u16::MAX - timer.counter) - 1);
                let ticks_to_overflow = u32::from(u16::MAX - timer.initial) + 1;
                #[allow(clippy::cast_possible_truncation)]
                let new_counter = timer.initial + (extra_ticks % ticks_to_overflow) as u16;
                #[allow(clippy::cast_possible_truncation)]
                let overflow_count = 1 + (extra_ticks / ticks_to_overflow) as u8;

                if timer.irq_enabled {
                    irq.request(match i {
                        0 => Interrupt::Timer0,
                        1 => Interrupt::Timer1,
                        2 => Interrupt::Timer2,
                        3 => Interrupt::Timer3,
                        _ => unreachable!(),
                    });
                }
                audio.notify_timer_overflow(i, overflow_count);
                prev_overflow_count = overflow_count.into();

                new_counter
            } else {
                new_counter
            };
        }
    }
}

impl Bus for Timers {
    fn read_byte(&mut self, addr: u32) -> u8 {
        assert!((0x100..0x110).contains(&addr), "IO register address OOB");

        let tmcnt = &mut self.0[(addr as usize & 0xf) / 4];
        #[allow(clippy::cast_possible_truncation)]
        match addr as usize & 3 {
            0 => tmcnt.counter as u8,
            1 => tmcnt.counter.bits(8..) as u8,
            2 => tmcnt.cached_bits as u8,
            3 => tmcnt.cached_bits.bits(8..) as u8,
            _ => unreachable!(),
        }
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        assert!((0x100..0x110).contains(&addr), "IO register address OOB");

        let tmcnt = &mut self.0[(addr as usize & 0xf) / 4];
        match addr as usize & 3 {
            0 => tmcnt.initial.set_bits(..8, value.into()),
            1 => tmcnt.initial.set_bits(8.., value.into()),
            2 => {
                tmcnt.cached_bits.set_bits(..8, value.into());
                tmcnt.frequency = value.bits(..2);
                tmcnt.cascade = value.bit(2);
                tmcnt.irq_enabled = value.bit(6);

                if !replace(&mut tmcnt.start, value.bit(7)) && tmcnt.start {
                    tmcnt.counter = tmcnt.initial;
                }
            }
            3 => tmcnt.cached_bits.set_bits(8.., value.into()),
            _ => unreachable!(),
        };
    }
}
