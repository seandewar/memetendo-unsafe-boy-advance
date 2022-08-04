use intbits::Bits;

pub mod noise;
pub mod tone;
pub mod wave;

const MAX_VOLUME: u8 = 15;

#[derive(Debug, Default)]
pub struct Length<const MAX_COUNTER: u16> {
    channel_enabled: bool,
    length_enabled: bool,
    counter: u16,
    initial: u16,
}

impl<const MAX_COUNTER: u16> Length<MAX_COUNTER> {
    pub fn step(&mut self) {
        if !self.length_enabled {
            return;
        }

        self.counter -= 1;
        if self.counter == 0 {
            self.channel_enabled = false;
        }
    }

    pub fn is_channel_enabled(&self) -> bool {
        self.channel_enabled
    }

    fn set_ctrl_byte(&mut self, idx: usize, value: u8) {
        match idx {
            0 => self.initial = u16::from(value) % MAX_COUNTER,
            1 => {
                self.length_enabled = value.bit(6);

                if value.bit(7) {
                    self.channel_enabled = true;
                    self.counter = if self.initial == 0 {
                        MAX_COUNTER
                    } else {
                        MAX_COUNTER - self.initial
                    };
                }
            }
            _ => unreachable!(),
        }
    }
}

#[derive(Debug, Default)]
pub struct LengthAndEnvelope {
    pub length: Length<64>,
    envelope_enabled: bool,
    envelope_volume: u8,
    envelope_initial_volume: u8,
    envelope_increase: bool,
    envelope_period: u8,
    envelope_clocks: u8,
}

impl LengthAndEnvelope {
    pub fn step_envelope(&mut self) {
        if !self.envelope_enabled || self.envelope_period == 0 {
            return;
        }

        self.envelope_clocks += 1;
        if self.envelope_clocks < self.envelope_period {
            return;
        }
        self.envelope_clocks = 0;

        if self.envelope_increase {
            if self.envelope_volume == MAX_VOLUME {
                self.envelope_enabled = false;
                return;
            }
            self.envelope_volume += 1;
        } else {
            if self.envelope_volume == 0 {
                self.envelope_enabled = false;
                return;
            }
            self.envelope_volume -= 1;
        }
    }

    fn volume(&self) -> u8 {
        if self.length.channel_enabled {
            self.envelope_volume
        } else {
            0
        }
    }

    fn set_ctrl_byte(&mut self, idx: usize, value: u8) {
        match idx {
            0 => self.length.set_ctrl_byte(0, value),
            1 => {
                self.envelope_period = value.bits(..3);
                self.envelope_increase = value.bit(3);
                self.envelope_initial_volume = value.bits(4..);
            }
            2 => {
                self.length.set_ctrl_byte(1, value);

                if value.bit(7) {
                    self.envelope_enabled = true;
                    self.envelope_clocks = 0;
                    self.envelope_volume = self.envelope_initial_volume;
                }
            }
            _ => unreachable!(),
        }
    }
}
