use std::mem::{replace, take};

use intbits::Bits;
use strum_macros::FromRepr;

use crate::{
    audio::Audio,
    bus::Bus,
    irq::{Interrupt, Irq},
};

#[derive(Debug, Default, FromRepr)]
#[repr(u8)]
enum PrescalarSelect {
    #[default]
    Div1,
    Div64,
    Div256,
    Div1024,
}

#[derive(Debug, Default)]
struct Control {
    accum: u32,
    initial: u16,
    counter: u16,
    prescalar_select: PrescalarSelect,
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

    // Panics if the ticks calculation overflows u16, but that shouldn't be possible.
    #[expect(clippy::missing_panics_doc)]
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

                    let shift = match timer.prescalar_select {
                        PrescalarSelect::Div1 => 10,
                        PrescalarSelect::Div64 => 4,
                        PrescalarSelect::Div256 => 2,
                        PrescalarSelect::Div1024 => 0,
                    };
                    timer.accum += u32::from(cycles) << shift;
                    if timer.accum < MAX_DIV {
                        continue;
                    }

                    let ticks = u16::try_from(timer.accum / MAX_DIV).unwrap();
                    timer.accum %= MAX_DIV;

                    ticks
                }
            };

            let (new_counter, overflowed) = timer.counter.overflowing_add(ticks);
            timer.counter = if overflowed {
                let extra_ticks = u32::from(ticks - (u16::MAX - timer.counter) - 1);
                let ticks_to_overflow = u32::from(u16::MAX - timer.initial) + 1;
                let new_counter =
                    timer.initial + u16::try_from(extra_ticks % ticks_to_overflow).unwrap();
                let overflow_count = 1 + u8::try_from(extra_ticks / ticks_to_overflow).unwrap();

                if timer.irq_enabled {
                    irq.request(
                        [
                            Interrupt::Timer0,
                            Interrupt::Timer1,
                            Interrupt::Timer2,
                            Interrupt::Timer3,
                        ][i],
                    );
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

        let tmcnt = &mut self.0[usize::try_from(addr & 0xf).unwrap() / 4];
        match addr & 3 {
            0 => tmcnt.counter.bits(..8).try_into().unwrap(),
            1 => tmcnt.counter.bits(8..).try_into().unwrap(),
            2 => tmcnt.cached_bits.bits(..8).try_into().unwrap(),
            3 => tmcnt.cached_bits.bits(8..).try_into().unwrap(),
            _ => unreachable!(),
        }
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        assert!((0x100..0x110).contains(&addr), "IO register address OOB");

        let tmcnt = &mut self.0[usize::try_from(addr & 0xf).unwrap() / 4];
        match addr & 3 {
            0 => tmcnt.initial.set_bits(..8, value.into()),
            1 => tmcnt.initial.set_bits(8.., value.into()),
            2 => {
                tmcnt.cached_bits.set_bits(..8, value.into());
                tmcnt.prescalar_select = PrescalarSelect::from_repr(value.bits(..2)).unwrap();
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
