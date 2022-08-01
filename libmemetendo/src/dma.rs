use std::mem::replace;

use intbits::Bits;

use crate::{
    bus::Bus,
    irq::{Interrupt, Irq},
};

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Default)]
struct Channel {
    init_src_addr: u32,
    init_dst_addr: u32,
    init_blocks: u32,
    src_addr_ctrl: u8,
    dst_addr_ctrl: u8,
    repeat: bool,
    transfer_word: bool,
    cart_drq: bool,
    timing_mode: u8,
    irq_enabled: bool,
    enabled: bool,
    cached_dmacnt_hi_bits: u16,

    curr_src_addr: u32,
    curr_dst_addr: u32,
    rem_blocks: u32,
    transferring: bool,
}

#[derive(Debug, Default)]
pub struct Dma([Channel; 4]);

impl Dma {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn start_transfer(&mut self, chan_idx: usize) {
        let chan = &mut self.0[chan_idx];
        if !chan.enabled || chan.transferring {
            return;
        }

        let max_blocks = if chan_idx == 3 { 0x1_0000 } else { 0x4000 };
        if chan.rem_blocks == 0 || chan.rem_blocks > max_blocks {
            chan.rem_blocks = max_blocks;
        }
        chan.transferring = true;
    }

    #[must_use]
    pub fn step<B: Bus>(&mut self, irq: &mut Irq, cycles: u8) -> Option<impl Fn(&mut B)> {
        // TODO: use cycles and transfer more than 1 block at a time if applicable,
        //       cart DRQ, special timing modes
        for chan_idx in 0..self.0.len() {
            if !self.0[chan_idx].enabled {
                continue;
            }
            if !self.0[chan_idx].transferring {
                if self.0[chan_idx].timing_mode != 0 {
                    continue;
                }
                self.start_transfer(chan_idx);
            }

            let chan = &mut self.0[chan_idx];
            let transfer_src_addr = chan.curr_src_addr;
            let transfer_dst_addr = chan.curr_dst_addr;
            let transfer_word = chan.transfer_word;

            let update_addr = |addr: &mut u32, ctrl| {
                let stride = if chan.transfer_word { 4 } else { 2 };
                match ctrl {
                    0 | 3 => *addr = addr.wrapping_add(stride),
                    1 => *addr = addr.wrapping_sub(stride),
                    2 => {}
                    _ => unreachable!(),
                };
            };
            update_addr(&mut chan.curr_src_addr, chan.src_addr_ctrl);
            update_addr(&mut chan.curr_dst_addr, chan.dst_addr_ctrl);

            chan.rem_blocks -= 1;
            if chan.rem_blocks == 0 {
                chan.transferring = false;
                chan.enabled = chan.repeat;
                if chan.repeat {
                    if chan.dst_addr_ctrl == 3 {
                        chan.curr_dst_addr = chan.init_dst_addr;
                    }
                    chan.rem_blocks = chan.init_blocks;
                }

                if chan.irq_enabled {
                    irq.request(match chan_idx {
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
        self.0.iter().any(|chan| chan.transferring)
    }
}

#[derive(Debug, Copy, Clone)]
pub enum Event {
    VBlank,
    HBlank,
}

impl Dma {
    pub fn notify(&mut self, event: Event) {
        let event_timing_mode = match event {
            Event::VBlank => 1,
            Event::HBlank => 2,
        };
        for chan_idx in 0..self.0.len() {
            if self.0[chan_idx].timing_mode == event_timing_mode {
                self.start_transfer(chan_idx);
            }
        }
    }
}

impl Bus for Dma {
    fn read_byte(&mut self, addr: u32) -> u8 {
        assert!((0xb0..0xe0).contains(&addr), "IO register address OOB");

        let chan = &mut self.0[(addr as usize - 0xb0) / 12];
        #[allow(clippy::cast_possible_truncation)]
        match (addr as usize - 0xb0) % 12 {
            // DMAXCNT
            10 => chan.cached_dmacnt_hi_bits as u8,
            11 => chan
                .cached_dmacnt_hi_bits
                .with_bit(15, chan.enabled)
                .bits(8..) as u8,
            _ => 0,
        }
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        assert!((0xb0..0xe0).contains(&addr), "IO register address OOB");

        let chan_idx = (addr as usize - 0xb0) / 12;
        let chan = &mut self.0[chan_idx];
        let offset = (addr as usize - 0xb0) % 12;

        let set_addr_byte = |addr: &mut u32, i, value: u8| match i {
            0..=2 => addr.set_bits((i * 8)..(i * 8) + 8, value.into()),
            3 if chan_idx == 0 => addr.set_bits(24.., value.bits(..3).into()),
            3 => addr.set_bits(24.., value.bits(..4).into()),
            _ => unreachable!(),
        };

        match offset {
            // DMAXSAD
            0..=3 => set_addr_byte(&mut chan.init_src_addr, offset & 3, value),
            // DMAXDAD
            4..=7 => set_addr_byte(&mut chan.init_dst_addr, offset & 3, value),
            // DMAXCNT
            8 => chan.init_blocks.set_bits(..8, value.into()),
            9 => chan.init_blocks.set_bits(8.., value.into()),
            10 => {
                chan.cached_dmacnt_hi_bits.set_bits(..8, value.into());
                chan.dst_addr_ctrl = value.bits(5..7);
                chan.src_addr_ctrl.set_bit(0, value.bit(7));
            }
            11 => {
                chan.cached_dmacnt_hi_bits.set_bits(8.., value.into());
                chan.src_addr_ctrl.set_bit(1, value.bit(0));
                chan.repeat = value.bit(1);
                chan.transfer_word = value.bit(2);
                chan.cart_drq = value.bit(3);
                chan.timing_mode = value.bits(4..6);
                chan.irq_enabled = value.bit(6);

                let old_enabled = replace(&mut chan.enabled, value.bit(7));
                if !old_enabled && chan.enabled {
                    chan.curr_src_addr = chan.init_src_addr;
                    chan.curr_dst_addr = chan.init_dst_addr;
                    chan.rem_blocks = chan.init_blocks;
                }
            }
            _ => unreachable!(),
        }
    }
}
