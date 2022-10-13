use intbits::Bits;

use crate::bus::Bus;

#[derive(Clone)]
pub struct Eeprom {
    buf: Box<[u8]>,
    state: State,
}

const BLOCK_LEN: usize = 64;

impl Eeprom {
    pub fn new(size_8k: bool) -> Self {
        Self {
            buf: vec![0xff; if size_8k { 128 } else { 8 } * BLOCK_LEN].into(),
            state: State::None,
        }
    }
}

#[derive(Default, Copy, Clone)]
enum State {
    #[default]
    None,
    Type,
    ReadAddress {
        block_idx: u16,
        bit_idx: usize,
    },
    ReadBlock {
        start_bit_idx: usize,
        rem_len: usize,
    },
    WriteAddress {
        block_idx: u16,
        bit_idx: usize,
    },
    WriteBlock {
        block_idx: u16,
        data: u64,
        bit_idx: usize,
    },
}

impl Bus for Eeprom {
    fn read_byte(&mut self, addr: u32) -> u8 {
        if addr % 2 == 1 {
            return 0;
        }

        0.with_bit(
            0,
            match &mut self.state {
                // Read 4 junk bits before a block.
                State::ReadBlock { rem_len, .. } if *rem_len > BLOCK_LEN => {
                    *rem_len -= 1;

                    false
                }
                // Read block.
                State::ReadBlock {
                    start_bit_idx,
                    rem_len,
                } => {
                    let bit_idx = *start_bit_idx + *rem_len - 1;
                    let byte_idx = bit_idx / 8;
                    let bit = if byte_idx < self.buf.len() {
                        self.buf[byte_idx].bit(bit_idx % 8)
                    } else {
                        false
                    };

                    *rem_len -= 1;
                    if *rem_len == 0 {
                        self.state = State::None;
                    }

                    bit
                }
                // Check status.
                State::None => true,
                State::Type
                | State::ReadAddress { .. }
                | State::WriteAddress { .. }
                | State::WriteBlock { .. } => false,
            },
        )
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        if addr % 2 == 1 {
            return;
        }

        let block_idx_bits = if self.buf.len() / BLOCK_LEN > 64 {
            14
        } else {
            6
        };
        match (&mut self.state, value.bit(0)) {
            (State::None, true) => self.state = State::Type,
            (State::None, false) | (State::ReadBlock { .. }, _) => {}
            (State::Type, true) => {
                self.state = State::ReadAddress {
                    block_idx: 0,
                    bit_idx: 0,
                };
            }
            (State::Type, false) => {
                self.state = State::WriteAddress {
                    block_idx: 0,
                    bit_idx: 0,
                };
            }
            // Address for block read.
            (State::ReadAddress { block_idx, bit_idx }, bit) if *bit_idx < block_idx_bits => {
                block_idx.set_bit(block_idx_bits - 1 - *bit_idx, bit);
                *bit_idx += 1;
            }
            // Read block. TODO: docs say this should be a 0 bit, but what if it's not?
            (State::ReadAddress { block_idx, .. }, _) => {
                self.state = State::ReadBlock {
                    start_bit_idx: usize::from(*block_idx) * BLOCK_LEN,
                    rem_len: BLOCK_LEN + 4, // First 4 bits should be ignored.
                }
            }
            // Address for block write.
            (State::WriteAddress { block_idx, bit_idx }, bit) => {
                block_idx.set_bit(block_idx_bits - 1 - *bit_idx, bit);
                *bit_idx += 1;
                if *bit_idx >= block_idx_bits {
                    self.state = State::WriteBlock {
                        block_idx: *block_idx,
                        data: 0,
                        bit_idx: 0,
                    }
                }
            }
            // Write block.
            (State::WriteBlock { data, bit_idx, .. }, bit) if *bit_idx < BLOCK_LEN => {
                data.set_bit(BLOCK_LEN - 1 - *bit_idx, bit);
                *bit_idx += 1;
            }
            // Commit written block. TODO: docs say this should be a 0 bit, but what if it's not?
            (
                State::WriteBlock {
                    data, block_idx, ..
                },
                _,
            ) => {
                // TODO: this should take ~108,368 cycles.
                let i = usize::from(*block_idx) * 8;
                if i + 8 < self.buf.len() {
                    self.buf[i..i + 8].copy_from_slice(&data.to_le_bytes());
                }
                self.state = State::None;
            }
        }
    }
}
