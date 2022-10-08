use std::mem::{replace, take};

use intbits::Bits;

use crate::{bus::Bus, dma::Dma, CYCLES_PER_SECOND};

use self::chan::{
    noise::Noise,
    tone::{Tone, ToneAndSweep},
    wave::{Fifo, Wave},
};

mod chan;

pub trait Callback {
    fn push_sample(&mut self, sample: (i16, i16));
}

#[derive(Debug, Default)]
pub struct Audio {
    channels: (ToneAndSweep, Tone, Wave, Noise, Fifo<true>, Fifo<false>),
    frame_seq_step: u8,
    frame_seq_cycle_accum: u16,
    freq_timer_cycles_accum: u16,
    fifo_pending_steps: [u8; 2],

    enabled: bool,
    out_channels: ([bool; 6], [bool; 6]),
    out_dmg_volume: (u8, u8),
    dmg_volume_ratio: u8,
    fifo_full_volume: [bool; 2],
    fifo_timer_idx: [usize; 2],
    bias: i16,
    sampling_cycle: u8,

    cached_soundcnt_bits: u64,
    cached_soundbias_bits: u64,
}

/// Right now, samples are outputted at the same rate that the frequency timer is emulated.
/// (currently very slightly slower than real hardware)
pub const SAMPLE_FREQUENCY: u32 = CYCLES_PER_SECOND / CYCLES_PER_SAMPLE as u32;
pub const CYCLES_PER_SAMPLE: u16 = CYCLES_PER_FREQ_TIMER_CLOCK;

// Frequency timer runs at 2,097,152 Hz.
const CYCLES_PER_FREQ_TIMER_CLOCK: u16 = (CYCLES_PER_SECOND / 2_097_152) as _;

impl Audio {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self, skip_bios: bool) {
        // TODO: proper resetting; for now, just reset SOUNDBIAS so audio doesn't suck
        if skip_bios {
            self.write_word(0x88, 0x0000_0200);
        }
    }

    pub fn step(&mut self, cb: &mut impl Callback, dma: &mut Dma, cycles: u8) {
        // Frame sequencer runs at 512 Hz.
        #[allow(clippy::cast_possible_truncation)] // it's fine clippy, gosh
        const CYCLES_PER_FRAME_SEQ_CLOCK: u16 = (CYCLES_PER_SECOND / 512) as _;

        if !self.enabled {
            return;
        }

        self.frame_seq_cycle_accum += u16::from(cycles);
        if self.frame_seq_cycle_accum >= CYCLES_PER_FRAME_SEQ_CLOCK {
            self.frame_seq_cycle_accum -= CYCLES_PER_FRAME_SEQ_CLOCK;

            if self.frame_seq_step % 2 == 0 {
                self.channels.0.length_and_envelope().length.step();
                self.channels.1.length_and_envelope.length.step();
                self.channels.2.length.step();
                self.channels.3.length_and_envelope.length.step();
            }
            if self.frame_seq_step == 2 || self.frame_seq_step == 6 {
                self.channels.0.step_sweep();
            } else if self.frame_seq_step == 7 {
                self.channels.0.length_and_envelope().step_envelope();
                self.channels.1.length_and_envelope.step_envelope();
                self.channels.3.length_and_envelope.step_envelope();
            }

            self.frame_seq_step += 1;
            self.frame_seq_step %= 8;
        }

        self.channels
            .4
            .step(dma, take(&mut self.fifo_pending_steps[0]));
        self.channels
            .5
            .step(dma, take(&mut self.fifo_pending_steps[1]));

        self.freq_timer_cycles_accum += u16::from(cycles);
        while self.freq_timer_cycles_accum >= CYCLES_PER_FREQ_TIMER_CLOCK {
            self.freq_timer_cycles_accum -= CYCLES_PER_FREQ_TIMER_CLOCK;

            self.channels.0.step_duty();
            self.channels.1.step_duty();
            self.channels.2.step_wave();
            self.channels.3.step_noise();

            cb.push_sample(self.mix_sample());
        }
    }

    fn mix_sample(&self) -> (i16, i16) {
        let mix_dmg = |out_channels: &[bool], out_volume| {
            let mut sum = 0;
            if out_channels[0] {
                sum += i16::from(self.channels.0.volume());
            }
            if out_channels[1] {
                sum += i16::from(self.channels.1.volume());
            }
            if out_channels[2] {
                sum += i16::from(self.channels.2.volume());
            }
            if out_channels[3] {
                sum += i16::from(self.channels.3.volume());
            }

            (sum * i16::from(out_volume + 1)) >> (2 - self.dmg_volume_ratio)
        };
        let mixed_dmg = (
            mix_dmg(&self.out_channels.0, self.out_dmg_volume.0),
            mix_dmg(&self.out_channels.1, self.out_dmg_volume.1),
        );

        let mix_fifo = |out_channels: &[bool]| {
            let mut sum = 0;
            if out_channels[4] {
                sum += (4 * i16::from(self.channels.4.sample()))
                    / if self.fifo_full_volume[0] { 1 } else { 2 };
            }
            if out_channels[5] {
                sum += (4 * i16::from(self.channels.5.sample()))
                    / if self.fifo_full_volume[1] { 1 } else { 2 };
            }

            sum
        };
        let mixed_fifo = (
            mix_fifo(&self.out_channels.0),
            mix_fifo(&self.out_channels.1),
        );

        let mut sample = (mixed_dmg.0 + mixed_fifo.0, mixed_dmg.1 + mixed_fifo.1);
        // Apply the bias, which is used to ensure the sample is clipped within the signed 10-bit
        // range for the DAC.
        sample.0 += self.bias;
        sample.1 += self.bias;
        sample.0 = sample.0.clamp(0, 0x3ff);
        sample.1 = sample.1.clamp(0, 0x3ff);
        sample.0 -= self.bias;
        sample.1 -= self.bias;

        // Scale to the i16 range.
        sample.0 = sample.0.saturating_mul(i16::MAX / 0x200);
        sample.1 = sample.1.saturating_mul(i16::MAX / 0x200);

        sample
    }

    pub fn notify_timer_overflow(&mut self, timer_idx: usize, count: u8) {
        if self.fifo_timer_idx[0] == timer_idx {
            self.fifo_pending_steps[0] += count;
        }
        if self.fifo_timer_idx[1] == timer_idx {
            self.fifo_pending_steps[1] += count;
        }
    }
}

impl Bus for Audio {
    fn read_byte(&mut self, addr: u32) -> u8 {
        let ctrl_offset = 8 * (addr as usize & 7);
        #[allow(clippy::cast_possible_truncation)]
        match addr {
            // SOUND1CNT
            0x60..=0x67 => self
                .channels
                .0
                .ctrl_bits()
                .bits(ctrl_offset..ctrl_offset + 8) as u8,
            // SOUND2CNT
            0x68..=0x6f => self
                .channels
                .1
                .ctrl_bits()
                .bits(ctrl_offset..ctrl_offset + 8) as u8,
            // SOUND3CNT
            0x70..=0x77 => self
                .channels
                .2
                .ctrl_bits()
                .bits(ctrl_offset..ctrl_offset + 8) as u8,
            // SOUND4CNT
            0x78..=0x7f => self
                .channels
                .3
                .ctrl_bits()
                .bits(ctrl_offset..ctrl_offset + 8) as u8,
            // SOUNDCNT
            0x80..=0x87 => {
                let cached_bits =
                    self.cached_soundcnt_bits.bits(ctrl_offset..ctrl_offset + 8) as u8;

                if addr == 0x84 {
                    cached_bits
                        .with_bit(
                            0,
                            self.channels
                                .0
                                .length_and_envelope()
                                .length
                                .is_channel_enabled(),
                        )
                        .with_bit(
                            1,
                            self.channels
                                .1
                                .length_and_envelope
                                .length
                                .is_channel_enabled(),
                        )
                        .with_bit(2, self.channels.2.length.is_channel_enabled())
                        .with_bit(
                            3,
                            self.channels
                                .3
                                .length_and_envelope
                                .length
                                .is_channel_enabled(),
                        )
                } else {
                    cached_bits
                }
            }
            // SOUNDBIAS
            0x88..=0x8f => self
                .cached_soundbias_bits
                .bits(ctrl_offset..ctrl_offset + 8) as u8,
            // WAVE_RAM
            0x90..=0x9f => self.channels.2.wave_ram().read_byte(addr & 0xf),
            0x00..=0x5f | 0xa8.. => panic!("IO register address OOB"),
            _ => 0,
        }
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        if !self.enabled && (0x60..=0x81).contains(&addr) {
            return;
        }

        let ctrl_offset = 8 * (addr as usize & 7);
        match addr {
            // SOUND1CNT
            0x60..=0x67 => self.channels.0.set_ctrl_byte(addr as usize & 7, value),
            // SOUND2CNT
            0x68..=0x6f => self.channels.1.set_ctrl_byte(addr as usize & 7, value),
            // SOUND3CNT
            0x70..=0x77 => self.channels.2.set_ctrl_byte(addr as usize & 7, value),
            // SOUND4CNT
            0x78..=0x7f => self.channels.3.set_ctrl_byte(addr as usize & 7, value),
            // SOUNDCNT
            0x80..=0x87 => {
                self.cached_soundcnt_bits
                    .set_bits(ctrl_offset..ctrl_offset + 8, value.into());

                match addr & 7 {
                    // SOUNDCNT_L
                    0 => {
                        self.out_dmg_volume.0 = value.bits(..3);
                        self.out_dmg_volume.1 = value.bits(4..7);
                    }
                    1 => {
                        self.out_channels.0[0] = value.bit(0);
                        self.out_channels.0[1] = value.bit(1);
                        self.out_channels.0[2] = value.bit(2);
                        self.out_channels.0[3] = value.bit(3);

                        self.out_channels.1[0] = value.bit(4);
                        self.out_channels.1[1] = value.bit(5);
                        self.out_channels.1[2] = value.bit(6);
                        self.out_channels.1[3] = value.bit(7);
                    }
                    // SOUNDCNT_H
                    2 => {
                        self.dmg_volume_ratio = value.bits(0..2).min(2);
                        self.fifo_full_volume[0] = value.bit(2);
                        self.fifo_full_volume[1] = value.bit(3);
                    }
                    3 => {
                        self.out_channels.1[4] = value.bit(0);
                        self.out_channels.0[4] = value.bit(1);
                        self.fifo_timer_idx[0] = value.bits(2..3).into();
                        if value.bit(3) {
                            self.channels.4.reset();
                        }

                        self.out_channels.1[5] = value.bit(4);
                        self.out_channels.0[5] = value.bit(5);
                        self.fifo_timer_idx[1] = value.bits(6..7).into();
                        if value.bit(7) {
                            self.channels.5.reset();
                        }
                    }
                    // SOUNDCNT_X
                    4 if replace(&mut self.enabled, value.bit(7)) && !self.enabled => {
                        for reg in 0x60..=0x81 {
                            self.write_byte(reg, 0);
                        }
                    }
                    _ => {}
                }
            }
            // SOUNDBIAS
            0x88..=0x8f => {
                self.cached_soundbias_bits
                    .set_bits(ctrl_offset..ctrl_offset + 8, value.into());

                match addr & 7 {
                    0 => self.bias.set_bits(..7, value.bits(1..).into()),
                    1 => {
                        self.bias.set_bits(7.., value.bits(..2).into());
                        self.sampling_cycle = value.bits(6..); // TODO: implement this?
                    }
                    _ => {}
                }
            }
            // WAVE_RAM
            0x90..=0x9f => self.channels.2.wave_ram().write_byte(addr & 0xf, value),
            // FIFO_A
            0xa0..=0xa3 => self.channels.4.write_byte(addr & 3, value),
            // FIFO_B
            0xa4..=0xa7 => self.channels.5.write_byte(addr & 3, value),
            0x00..=0x5f | 0xa8.. => panic!("IO register address OOB"),
        }
    }
}
