use intbits::Bits;

use super::LengthAndEnvelope;

#[derive(Debug, Default)]
pub struct Tone {
    pub length_and_envelope: LengthAndEnvelope,
    frequency: u16,
    duty_mode: u8,
    duty_step: u8,
    duty_step_clocks: u16,
    cached_bits: u64,
}

const MAX_FREQUENCY: u16 = 2047;

impl Tone {
    pub fn step_duty(&mut self) {
        self.duty_step_clocks += 1;
        if self.duty_step_clocks < 2 * (MAX_FREQUENCY + 1 - self.frequency) {
            return;
        }
        self.duty_step_clocks = 0;

        self.duty_step += 1;
        self.duty_step %= 8;
    }

    pub fn volume(&self) -> u8 {
        // 8 total steps per duty cycle.
        if self.duty_step < [1, 2, 4, 6][usize::from(self.duty_mode)] {
            self.length_and_envelope.volume()
        } else {
            0
        }
    }

    pub fn set_ctrl_byte(&mut self, idx: usize, value: u8) {
        match idx {
            // SOUND2CNT_L
            0 => {
                self.cached_bits.set_bits(..8, value.into());
                self.length_and_envelope.set_ctrl_byte(0, value);
                self.duty_mode = value.bits(6..);
            }
            1 => {
                self.cached_bits.set_bits(8..16, value.into());
                self.length_and_envelope.set_ctrl_byte(1, value);
            }
            // Unused
            2 => self.cached_bits.set_bits(16..24, value.into()),
            3 => self.cached_bits.set_bits(24..32, value.into()),
            // SOUND2CNT_H
            4 => {
                self.cached_bits.set_bits(32..40, value.into());
                self.frequency.set_bits(..8, value.into());
            }
            5 => {
                self.cached_bits.set_bits(40..48, value.into());
                self.length_and_envelope.set_ctrl_byte(2, value);
                self.frequency.set_bits(8.., value.bits(..3).into());

                if value.bit(7) {
                    self.duty_step_clocks = 0;
                }
            }
            // Unused
            6 => self.cached_bits.set_bits(48..56, value.into()),
            7 => self.cached_bits.set_bits(56.., value.into()),
            _ => unreachable!(),
        }
    }

    pub fn ctrl_bits(&self) -> u64 {
        self.cached_bits
    }
}

#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Default)]
pub struct ToneAndSweep {
    tone: Tone,
    sweep_enabled: bool,
    sweep_shadow_frequency: u16,
    sweep_shift: u8,
    sweep_decrease: bool,
    sweep_period: u8,
    sweep_clocks: u8,
    cached_bits: u64,
}

impl ToneAndSweep {
    pub fn step_sweep(&mut self) {
        if !self.sweep_enabled || self.sweep_period == 0 {
            return;
        }

        self.sweep_clocks += 1;
        if self.sweep_clocks < self.sweep_period {
            return;
        }
        self.sweep_clocks = 0;

        let update_shadow_freq = |chan: &mut Self| {
            let offset = chan.sweep_shadow_frequency >> chan.sweep_shift;
            if chan.sweep_decrease {
                if offset <= chan.sweep_shadow_frequency {
                    chan.sweep_shadow_frequency -= offset;
                }
            } else {
                if chan.sweep_shadow_frequency + offset > MAX_FREQUENCY {
                    chan.tone.length_and_envelope.length.channel_enabled = false;
                    return;
                }
                chan.sweep_shadow_frequency += offset;
            }
        };

        update_shadow_freq(self);
        if !self.tone.length_and_envelope.length.channel_enabled {
            return;
        }
        self.tone.frequency = self.sweep_shadow_frequency;

        // Update again to mimic GB behaviour, but don't update the current frequency.
        update_shadow_freq(self);
    }

    pub fn step_duty(&mut self) {
        self.tone.step_duty();
    }

    pub fn volume(&self) -> u8 {
        self.tone.volume()
    }

    pub fn length_and_envelope(&mut self) -> &mut LengthAndEnvelope {
        &mut self.tone.length_and_envelope
    }

    pub fn set_ctrl_byte(&mut self, idx: usize, value: u8) {
        match idx {
            // SOUND1CNT_L
            0 => {
                self.cached_bits.set_bits(..8, value.into());
                self.sweep_shift = value.bits(..3);
                self.sweep_decrease = value.bit(3);
                self.sweep_period = value.bits(4..7);
            }
            1 => self.cached_bits.set_bits(8..16, value.into()),
            // SOUND1CNT_H
            2 => {
                self.cached_bits.set_bits(16..24, value.into());
                self.tone.set_ctrl_byte(0, value);
            }
            3 => {
                self.cached_bits.set_bits(24..32, value.into());
                self.tone.set_ctrl_byte(1, value);
            }
            // SOUND1CNT_X
            4 => {
                self.cached_bits.set_bits(32..40, value.into());
                self.tone.set_ctrl_byte(4, value);
            }
            5 => {
                self.cached_bits.set_bits(40..48, value.into());
                self.tone.set_ctrl_byte(5, value);

                if value.bit(7) {
                    self.sweep_enabled = self.sweep_period > 0 || self.sweep_shift > 0;
                    self.sweep_shadow_frequency = self.tone.frequency;
                    self.sweep_clocks = 0;

                    if self.sweep_shift > 0 {
                        self.step_sweep();
                    }
                }
            }
            // Unused
            6 => self.cached_bits.set_bits(48..56, value.into()),
            7 => self.cached_bits.set_bits(56.., value.into()),
            _ => unreachable!(),
        }
    }

    pub fn ctrl_bits(&self) -> u64 {
        self.cached_bits
    }
}
