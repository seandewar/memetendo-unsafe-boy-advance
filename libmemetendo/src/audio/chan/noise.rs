use intbits::Bits;

use super::LengthAndEnvelope;

#[derive(Debug)]
pub struct Noise {
    pub length_and_envelope: LengthAndEnvelope,
    lfsr: u16,
    half_width: bool,
    period: u8,
    period_shift: u8,
    clocks: u8,
    cached_bits: u64,
}

impl Default for Noise {
    fn default() -> Self {
        Self {
            length_and_envelope: LengthAndEnvelope::default(),
            lfsr: 0x7fff,
            half_width: false,
            period: 0,
            period_shift: 0,
            clocks: 0,
            cached_bits: 0,
        }
    }
}

impl Noise {
    pub fn step_noise(&mut self) {
        self.clocks += 1;
        let period = [8, 16, 32, 48, 64, 80, 96, 112][usize::from(self.period)];
        if self.clocks < period >> self.period_shift {
            return;
        }
        self.clocks = 0;

        let xor = self.lfsr.bit(0) ^ self.lfsr.bit(1);
        self.lfsr >>= 1;
        self.lfsr.set_bit(14, xor);
        if self.half_width {
            self.lfsr.set_bit(6, xor);
        }
    }

    pub fn volume(&self) -> u8 {
        if self.lfsr.bit(0) {
            0
        } else {
            self.length_and_envelope.volume()
        }
    }

    pub fn set_ctrl_byte(&mut self, idx: usize, value: u8) {
        match idx {
            // SOUND4CNT_L
            0 => {
                self.cached_bits.set_bits(..8, value.into());
                self.length_and_envelope.set_ctrl_byte(0, value);
            }
            1 => {
                self.cached_bits.set_bits(8..16, value.into());
                self.length_and_envelope.set_ctrl_byte(1, value);
            }
            // Unused
            2 => self.cached_bits.set_bits(16..24, value.into()),
            3 => self.cached_bits.set_bits(24..32, value.into()),
            // SOUND4CNT_H
            4 => {
                self.cached_bits.set_bits(32..40, value.into());
                self.period = value.bits(..3);
                self.half_width = value.bit(3);
                self.period_shift = value.bits(4..);
            }
            5 => {
                self.cached_bits.set_bits(40..48, value.into());
                self.length_and_envelope.set_ctrl_byte(2, value);
            }
            6 => self.cached_bits.set_bits(48..56, value.into()),
            7 => self.cached_bits.set_bits(56.., value.into()),
            _ => unreachable!(),
        }
    }

    pub fn ctrl_bits(&self) -> u64 {
        self.cached_bits
    }
}
