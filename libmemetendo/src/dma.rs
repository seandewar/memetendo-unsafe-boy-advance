use std::mem::take;

use intbits::Bits;
use strum::EnumCount;
use strum_macros::EnumCount;

use crate::{
    bus::Bus,
    irq::{Interrupt, Irq},
};

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Default)]
pub struct Registers {
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
    pub reg: [Registers; 4],
    pending_events: [bool; Event::COUNT],
}

impl Dmas {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn step<B: Bus>(&mut self, irq: &mut Irq, cycles: u32) -> Option<impl Fn(&mut B)> {
        // TODO: use cycles and transfer more than 1 block, cart DRQ, special timing modes
        let mut transfer_fn = None;
        for (i, dma) in self.reg.iter_mut().enumerate() {
            if !dma.enabled {
                continue;
            }
            if !dma.transferring {
                let start_transfer = match dma.timing_mode {
                    0 => true,
                    1 => self.pending_events[Event::VBlank as usize],
                    2 => self.pending_events[Event::HBlank as usize],
                    // Special: DMA0: Prohibited, DMA1/2: Sound FIFO, DMA3: Video Capture
                    3 => false, // TODO
                    _ => unreachable!(),
                };
                if !start_transfer {
                    continue;
                }

                dma.transferring = true;
                if dma.rem_blocks == 0 {
                    dma.rem_blocks = if i == 3 { 0x1_0000 } else { 0x4000 };
                }
            }

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

            transfer_fn = Some(move |bus: &mut B| {
                if transfer_word {
                    let value = bus.read_word(transfer_src_addr);
                    bus.write_word(transfer_dst_addr, value);
                } else {
                    let value = bus.read_hword(transfer_src_addr);
                    bus.write_hword(transfer_dst_addr, value);
                }
            });
            break;
        }

        take(&mut self.pending_events);

        transfer_fn
    }

    #[must_use]
    pub fn transfer_in_progress(&self) -> bool {
        self.reg.iter().any(|dma| dma.transferring)
    }
}

#[derive(Debug, EnumCount)]
pub enum Event {
    VBlank,
    HBlank,
}

impl Dmas {
    pub fn notify(&mut self, event: Event) {
        self.pending_events[event as usize] = true;
    }
}
