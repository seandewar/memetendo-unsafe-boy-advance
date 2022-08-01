use intbits::Bits;

use crate::{
    bus::Bus,
    irq::{Interrupt, Irq},
};

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Default)]
struct Registers {
    init_src_addr: u32,
    init_dst_addr: u32,
    init_blocks: u32,
    src_addr_control: u8,
    dst_addr_control: u8,
    pub repeat: bool,
    pub transfer_word: bool,
    pub cart_drq: bool,
    timing_mode: u8,
    pub irq_enabled: bool,
    enabled: bool,
    control_lo_bits: u8,

    curr_src_addr: u32,
    curr_dst_addr: u32,
    rem_blocks: u32,
    transferring: bool,
}

impl Registers {
    fn set_addr_byte(addr: &mut u32, idx: usize, value: u8) {
        match idx {
            0 => addr.set_bits(..8, value.into()),
            1 => addr.set_bits(8..16, value.into()),
            2 => addr.set_bits(16..24, value.into()),
            3 => addr.set_bits(24..28, value.bits(..4).into()),
            _ => panic!("byte index out of bounds"),
        }
    }

    pub fn set_src_addr_byte(&mut self, idx: usize, value: u8) {
        Self::set_addr_byte(&mut self.init_src_addr, idx, value);
    }

    pub fn set_dst_addr_byte(&mut self, idx: usize, value: u8) {
        Self::set_addr_byte(&mut self.init_dst_addr, idx, value);
    }

    pub fn set_size_lo_bits(&mut self, bits: u8) {
        self.init_blocks.set_bits(..8, bits.into());
    }

    pub fn set_size_hi_bits(&mut self, bits: u8) {
        self.init_blocks.set_bits(8.., bits.into());
    }

    pub fn set_control_lo_bits(&mut self, bits: u8) {
        self.dst_addr_control = bits.bits(5..7);
        self.src_addr_control.set_bit(0, bits.bit(7));
        self.control_lo_bits = bits;
    }

    pub fn set_control_hi_bits(&mut self, bits: u8) {
        self.src_addr_control.set_bit(1, bits.bit(0));
        self.repeat = bits.bit(1);
        self.transfer_word = bits.bit(2);
        self.cart_drq = bits.bit(3);
        self.timing_mode = bits.bits(4..6);
        self.irq_enabled = bits.bit(6);

        let old_enabled = self.enabled;
        self.enabled = bits.bit(7);
        if !old_enabled && self.enabled {
            self.curr_src_addr = self.init_src_addr;
            self.curr_dst_addr = self.init_dst_addr;
            self.rem_blocks = self.init_blocks;
        }
    }

    #[must_use]
    pub fn control_lo_bits(&self) -> u8 {
        self.control_lo_bits
    }

    #[must_use]
    pub fn control_hi_bits(&self) -> u8 {
        let mut bits = 0;
        bits.set_bit(0, self.src_addr_control.bit(1));
        bits.set_bit(1, self.repeat);
        bits.set_bit(2, self.transfer_word);
        bits.set_bit(3, self.cart_drq);
        bits.set_bits(4..6, self.timing_mode);
        bits.set_bit(6, self.irq_enabled);
        bits.set_bit(7, self.enabled);

        bits
    }
}

#[derive(Debug, Default)]
pub struct Dmas {
    reg: [Registers; 4],
}

impl Dmas {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn start_transfer(&mut self, dma_idx: usize) {
        let dma = &mut self.reg[dma_idx];
        if !dma.enabled || dma.transferring {
            return;
        }

        dma.transferring = true;
        if dma.rem_blocks == 0 {
            dma.rem_blocks = if dma_idx == 3 { 0x1_0000 } else { 0x4000 };
        }
    }

    #[must_use]
    pub fn step<B: Bus>(&mut self, irq: &mut Irq, cycles: u32) -> Option<impl Fn(&mut B)> {
        // TODO: max limit normal values of init_blocks, use cycles and transfer more than 1 block,
        //       cart DRQ, special timing modes
        for i in 0..self.reg.len() {
            if !self.reg[i].enabled {
                continue;
            }

            if !self.reg[i].transferring {
                if self.reg[i].timing_mode == 0 {
                    self.start_transfer(i);
                } else {
                    continue;
                }
            }

            let dma = &mut self.reg[i];
            let addr_bits = if i == 0 { 27 } else { 28 };
            let transfer_src_addr = dma.curr_src_addr.bits(..addr_bits);
            let transfer_dst_addr = dma.curr_dst_addr.bits(..addr_bits);
            let transfer_word = dma.transfer_word;

            let update_addr = |addr: &mut u32, control| {
                let stride = if dma.transfer_word { 4 } else { 2 };
                match control {
                    0 | 3 => *addr = addr.wrapping_add(stride),
                    1 => *addr = addr.wrapping_sub(stride),
                    2 => {}
                    _ => unreachable!(),
                };
            };
            update_addr(&mut dma.curr_src_addr, dma.src_addr_control);
            update_addr(&mut dma.curr_dst_addr, dma.dst_addr_control);

            dma.rem_blocks -= 1;
            if dma.rem_blocks == 0 {
                dma.transferring = false;
                dma.enabled = dma.repeat;
                if dma.repeat {
                    if dma.dst_addr_control == 3 {
                        dma.curr_dst_addr = dma.init_dst_addr;
                    }
                    dma.rem_blocks = dma.init_blocks;
                }

                if dma.irq_enabled {
                    irq.request(match i {
                        0 => Interrupt::Dma0,
                        1 => Interrupt::Dma1,
                        2 => Interrupt::Dma2,
                        3 => Interrupt::Dma3,
                        _ => unreachable!(),
                    });
                }
            }

            return Some(move |bus: &mut B| {
                if transfer_word {
                    let value = bus.read_word(transfer_src_addr);
                    bus.write_word(transfer_dst_addr, value);
                } else {
                    let value = bus.read_hword(transfer_src_addr);
                    bus.write_hword(transfer_dst_addr, value);
                }
            });
        }

        None
    }

    #[must_use]
    pub fn transfer_in_progress(&self) -> bool {
        self.reg.iter().any(|dma| dma.transferring)
    }
}

#[derive(Debug, Copy, Clone)]
pub enum Event {
    VBlank,
    HBlank,
}

impl Dmas {
    pub fn notify(&mut self, event: Event) {
        let event_timing_mode = match event {
            Event::VBlank => 1,
            Event::HBlank => 2,
        };

        for i in 0..self.reg.len() {
            if self.reg[i].timing_mode == event_timing_mode {
                self.start_transfer(i);
            }
        }
    }
}

impl Bus for Dmas {
    fn read_byte(&mut self, addr: u32) -> u8 {
        match addr {
            // DMA0CNT
            0xba => self.reg[0].control_lo_bits(),
            0xbb => self.reg[0].control_hi_bits(),
            // DMA1CNT
            0xc6 => self.reg[1].control_lo_bits(),
            0xc7 => self.reg[1].control_hi_bits(),
            // DMA2CNT
            0xd2 => self.reg[2].control_lo_bits(),
            0xd3 => self.reg[2].control_hi_bits(),
            // DMA3CNT
            0xde => self.reg[3].control_lo_bits(),
            0xdf => self.reg[3].control_hi_bits(),
            0x0..=0xaf | 0xe0.. => panic!("IO register address OOB"),
            _ => 0,
        }
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        match addr {
            // DMA0SAD
            0xb0..=0xb3 => self.reg[0].set_src_addr_byte((addr & 3) as usize, value),
            // DMA0DAD
            0xb4..=0xb7 => self.reg[0].set_dst_addr_byte((addr & 3) as usize, value),
            // DMA0CNT
            0xb8 => self.reg[0].set_size_lo_bits(value),
            0xb9 => self.reg[0].set_size_hi_bits(value),
            0xba => self.reg[0].set_control_lo_bits(value),
            0xbb => self.reg[0].set_control_hi_bits(value),
            // DMA1SAD
            0xbc..=0xbf => self.reg[1].set_src_addr_byte((addr & 3) as usize, value),
            // DMA1DAD
            0xc0..=0xc3 => self.reg[1].set_dst_addr_byte((addr & 3) as usize, value),
            // DMA1CNT
            0xc4 => self.reg[1].set_size_lo_bits(value),
            0xc5 => self.reg[1].set_size_hi_bits(value),
            0xc6 => self.reg[1].set_control_lo_bits(value),
            0xc7 => self.reg[1].set_control_hi_bits(value),
            // DMA2SAD
            0xc8..=0xcb => self.reg[2].set_src_addr_byte((addr & 3) as usize, value),
            // DMA2DAD
            0xcc..=0xcf => self.reg[2].set_dst_addr_byte((addr & 3) as usize, value),
            // DMA2CNT
            0xd0 => self.reg[2].set_size_lo_bits(value),
            0xd1 => self.reg[2].set_size_hi_bits(value),
            0xd2 => self.reg[2].set_control_lo_bits(value),
            0xd3 => self.reg[2].set_control_hi_bits(value),
            // DMA3SAD
            0xd4..=0xd7 => self.reg[3].set_src_addr_byte((addr & 3) as usize, value),
            // DMA3DAD
            0xd8..=0xdb => self.reg[3].set_dst_addr_byte((addr & 3) as usize, value),
            // DMA3CNT
            0xdc => self.reg[3].set_size_lo_bits(value),
            0xdd => self.reg[3].set_size_hi_bits(value),
            0xde => self.reg[3].set_control_lo_bits(value),
            0xdf => self.reg[3].set_control_hi_bits(value),
            _ => panic!("IO register address OOB"),
        }
    }
}
