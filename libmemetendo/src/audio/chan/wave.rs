use intbits::Bits;

use crate::{
    bus::Bus,
    dma::{Dma, Event},
};

use super::Length;

const WAVE_RAM_BANK_LEN: usize = 16;

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Default)]
pub struct Wave {
    pub length: Length<256>,
    wave_ram_banks: [[u8; WAVE_RAM_BANK_LEN]; 2],
    two_banks: bool,
    bank_idx: usize,
    bank_initial_idx: usize,
    play: bool,
    sample_rate: u16,
    sample_idx: usize,
    volume: u8,
    force_75_volume: bool,
    clocks: u16,
    cached_bits: u64,
}

#[allow(clippy::module_name_repetitions)]
pub struct WaveRam<'a>(&'a mut Wave);

// TODO: As the wave RAM is basically one giant shift register, reads and writes may be shifted,
//       but is this worth implementing?
impl Bus for WaveRam<'_> {
    fn read_byte(&mut self, addr: u32) -> u8 {
        self.0.wave_ram_banks[(self.0.bank_idx + 1) % 2][usize::try_from(addr).unwrap()]
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        self.0.wave_ram_banks[(self.0.bank_idx + 1) % 2][usize::try_from(addr).unwrap()] = value;
    }
}

impl Wave {
    pub fn step_wave(&mut self) {
        if !self.length.channel_enabled || !self.play {
            return;
        }

        self.clocks += 1;
        if self.clocks < 2048 - self.sample_rate {
            return;
        }
        self.clocks = 0;

        self.sample_idx += 1;
        if self.sample_idx >= WAVE_RAM_BANK_LEN {
            if !self.two_banks {
                self.bank_idx += 1;
                self.bank_idx %= 2;
            }
            self.sample_idx = 0;
        }
    }

    pub fn volume(&self) -> u8 {
        if self.length.channel_enabled && self.play {
            let bit_idx = 4 * (self.sample_idx % 2);
            let sample =
                self.wave_ram_banks[self.bank_idx][self.sample_idx / 2].bits(bit_idx..bit_idx + 4);

            if self.force_75_volume {
                sample - (sample / 4)
            } else if self.volume == 0 {
                0
            } else {
                sample >> (self.volume - 1)
            }
        } else {
            0
        }
    }

    pub fn set_ctrl_byte(&mut self, idx: usize, value: u8) {
        match idx {
            // SOUND3CNT_L
            0 => {
                self.cached_bits.set_bits(..8, value.into());
                self.two_banks = value.bit(5);
                self.bank_initial_idx = value.bits(6..7).into();
                self.play = value.bit(7);

                // TODO: is this correct?
                self.bank_idx = self.bank_initial_idx;
                self.sample_idx = 0;
            }
            1 => self.cached_bits.set_bits(8..16, value.into()),
            // SOUND3CNT_H
            2 => {
                self.cached_bits.set_bits(16..24, value.into());
                self.length.set_ctrl_byte(0, value);
            }
            3 => {
                self.cached_bits.set_bits(24..32, value.into());
                self.volume = value.bits(5..7);
                self.force_75_volume = value.bit(7);
            }
            // SOUND3CNT_X
            4 => {
                self.cached_bits.set_bits(32..40, value.into());
                self.sample_rate.set_bits(..8, value.into());
            }
            5 => {
                self.cached_bits.set_bits(40..48, value.into());
                self.length.set_ctrl_byte(1, value);
                self.sample_rate.set_bits(8..11, value.bits(..3).into());

                if value.bit(7) {
                    self.bank_idx = self.bank_initial_idx;
                    self.sample_idx = 0;
                }
            }
            6 => self.cached_bits.set_bits(48..56, value.into()),
            7 => self.cached_bits.set_bits(56.., value.into()),
            _ => unreachable!(),
        }
    }

    pub fn ctrl_bits(&self) -> u64 {
        self.cached_bits
    }

    pub fn wave_ram(&mut self) -> WaveRam {
        WaveRam(self)
    }
}

#[derive(Debug, Default)]
pub struct Fifo<const FIFO_A: bool> {
    sample: i8,
    samples: [i8; 32],
    start_idx: usize,
    len: usize,
}

impl<const FIFO_A: bool> Fifo<FIFO_A> {
    pub fn step(&mut self, dma: &mut Dma, steps: u8) {
        if steps == 0 {
            return;
        }

        self.sample = if self.len > 0 {
            let mut sample_accum = 0;

            let count = usize::from(steps).min(self.len);
            for _ in 0..count {
                sample_accum += i32::from(self.samples[self.start_idx]);
                self.start_idx += 1;
                self.start_idx %= self.samples.len();
            }
            self.len -= count;

            i8::try_from(sample_accum / i32::from(steps)).unwrap()
        } else {
            0
        };

        if self.len <= 16 {
            dma.notify(if FIFO_A {
                Event::AudioFifoA
            } else {
                Event::AudioFifoB
            });
        }
    }

    pub fn sample(&self) -> i8 {
        self.sample
    }

    pub fn reset(&mut self) {
        self.sample = 0;
        self.start_idx = 0;
        self.len = 0;
    }
}

impl<const FIFO_A: bool> Bus for Fifo<FIFO_A> {
    fn read_byte(&mut self, _addr: u32) -> u8 {
        0xff // TODO: Unpredictable on real hardware
    }

    fn write_byte(&mut self, _addr: u32, value: u8) {
        #[allow(clippy::cast_possible_wrap)]
        let sample = value as i8;
        if self.len < self.samples.len() {
            self.samples[(self.start_idx + self.len) % self.samples.len()] = sample;
            self.len += 1;
        } else {
            // Overwrite the oldest sample.
            // TODO: real hardware resets the FIFO here, but our timings aren't perfect and this
            // causes some games to hang (e.g: fifo overflow -> fifo clear -> fifo dma -> repeat)
            self.samples[self.start_idx] = sample;
            self.start_idx += 1;
            self.start_idx %= self.samples.len();
        }
    }
}
