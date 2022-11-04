use crate::bus::Bus;

#[derive(Clone)]
pub struct Flash {
    buf: Box<[u8]>,
    bank_idx: usize,
    state: State,
    next_cmd_state: NextCommandState,
}

#[derive(Default, Copy, Clone, Eq, PartialEq)]
enum State {
    #[default]
    None,
    Identify,
    Erase,
    Write,
    SwitchBank,
}

#[derive(Default, Copy, Clone, Eq, PartialEq)]
enum NextCommandState {
    #[default]
    None,
    Incomplete,
    Type,
}

const BANK_LEN: usize = 0x1_0000;

impl TryFrom<&mut Option<Box<[u8]>>> for Flash {
    type Error = ();

    fn try_from(buf: &mut Option<Box<[u8]>>) -> Result<Self, Self::Error> {
        Ok(Self {
            buf: match buf {
                Some(b) if b.len() == BANK_LEN || b.len() == 2 * BANK_LEN => buf.take().unwrap(),
                _ => return Err(()),
            },
            bank_idx: 0,
            state: State::None,
            next_cmd_state: NextCommandState::None,
        })
    }
}

impl Flash {
    pub fn new(dual_bank: bool) -> Self {
        Self::try_from(&mut Some(
            vec![0xff; if dual_bank { 2 } else { 1 } * BANK_LEN].into_boxed_slice(),
        ))
        .unwrap()
    }

    fn buf_index(&self, addr: u32) -> usize {
        self.bank_idx * BANK_LEN + addr as usize
    }

    fn is_dual_bank(&self) -> bool {
        self.buf.len() > BANK_LEN
    }

    pub fn buffer(&self) -> &[u8] {
        &self.buf
    }
}

impl Bus for Flash {
    fn read_byte(&mut self, addr: u32) -> u8 {
        match (addr, self.state) {
            // Identify as a Sanyo chip for dual bank.
            (0, State::Identify) if self.is_dual_bank() => 0x62,
            (1, State::Identify) if self.is_dual_bank() => 0x13,
            // Identify as an SST chip for single bank.
            (0, State::Identify) => 0xbf,
            (1, State::Identify) => 0xd4,
            (0..=0xffff, _) => self.buf[self.buf_index(addr)],
            _ => 0,
        }
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        if addr as usize >= BANK_LEN {
            return;
        }

        // TODO: how long do most of these write/erase operations take?
        match (self.state, self.next_cmd_state, addr, value) {
            // Write byte.
            (State::Write, _, _, _) => {
                self.buf[self.buf_index(addr)] = value;
                self.state = State::None;
            }
            // Erase 4KiB sector.
            (State::Erase, NextCommandState::Type, _, 0x30) if addr % 0x1000 == 0 => {
                let i = self.buf_index(addr);
                self.buf[i..i + 0x1000].fill(0xff);
                self.state = State::None;
                self.next_cmd_state = NextCommandState::None;
            }
            // Switch 64KiB bank.
            (State::SwitchBank, _, 0x0000, _) => {
                self.bank_idx = value.into();
                self.state = State::None;
            }
            (_, NextCommandState::Type, 0x5555, _) => {
                match (self.state, value) {
                    // Erase all.
                    (State::Erase, 0x10) => {
                        self.buf.fill(0xff);
                        self.state = State::None;
                    }
                    (State::None, 0x80) => self.state = State::Erase,
                    (State::None, 0x90) => self.state = State::Identify,
                    (State::None, 0xa0) => self.state = State::Write,
                    (State::None, 0xb0) if self.is_dual_bank() => self.state = State::SwitchBank,
                    (State::Identify, 0xf0) => self.state = State::None,
                    _ => {}
                }
                self.next_cmd_state = NextCommandState::None;
            }
            (_, NextCommandState::None, 0x5555, 0xaa) => {
                self.next_cmd_state = NextCommandState::Incomplete;
            }
            (_, NextCommandState::Incomplete, 0x2aaa, 0x55) => {
                self.next_cmd_state = NextCommandState::Type;
            }
            _ => {}
        }
    }
}
