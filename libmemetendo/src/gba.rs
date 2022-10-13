use intbits::Bits;

use crate::{
    arm7tdmi::Cpu,
    audio::{self, Audio},
    bios::Bios,
    bus,
    cart::Cartridge,
    dma::Dma,
    irq::Irq,
    keypad::Keypad,
    timer::Timers,
    video::{screen::Screen, Video},
};

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub enum State {
    #[default]
    Running,
    Halted,
    Stopped,
}

#[derive(Debug, Default)]
pub struct HaltControl(pub State);

impl HaltControl {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl bus::Bus for HaltControl {
    fn read_byte(&mut self, addr: u32) -> u8 {
        assert_eq!(addr, 0x301, "IO register address OOB");

        0
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        assert_eq!(addr, 0x301, "IO register address OOB");

        self.0 = if value.bit(7) {
            State::Stopped
        } else {
            State::Halted
        }
    }
}

pub struct Gba<'b, 'c> {
    pub cpu: Cpu,
    pub irq: Irq,
    pub haltcnt: HaltControl,
    pub timers: Timers,
    pub dma: Dma,
    pub iwram: Box<[u8]>,
    pub ewram: Box<[u8]>,
    pub video: Video,
    pub audio: Audio,
    pub keypad: Keypad,
    pub bios: Bios<'b>,
    pub cart: Cartridge<'c>,
    io_todo: Box<[u8]>,
}

impl<'b, 'c> Gba<'b, 'c> {
    #[must_use]
    pub fn new(bios: Bios<'b>, cart: Cartridge<'c>) -> Self {
        Self {
            cpu: Cpu::new(),
            irq: Irq::new(),
            haltcnt: HaltControl::new(),
            timers: Timers::new(),
            dma: Dma::new(),
            iwram: vec![0; 0x8000].into_boxed_slice(),
            ewram: vec![0; 0x40000].into_boxed_slice(),
            video: Video::new(),
            audio: Audio::new(),
            keypad: Keypad::new(),
            bios,
            cart,
            io_todo: vec![0; 0x801].into_boxed_slice(),
        }
    }

    pub fn reset(&mut self, skip_bios: bool) {
        // TODO: reset other hardware components
        self.bios.reset();
        self.cpu.reset(&mut bus!(self), skip_bios);
        self.audio.reset(skip_bios);

        if skip_bios {
            self.iwram[0x7e00..].fill(0);
            self.bios.update_protection(Some(0xdc + 8));
        }
    }

    pub fn step(
        &mut self,
        screen: &mut impl Screen,
        audio_cb: &mut impl audio::Callback,
        skip_drawing: bool,
    ) {
        self.keypad.step(&mut self.irq);

        if self.haltcnt.0 == State::Running && !self.dma.transfer_in_progress() {
            self.cpu.step(&mut bus!(self));
        }
        if self.haltcnt.0 != State::Stopped {
            // TODO: actual cycle counting
            self.video
                .step(screen, &mut self.irq, &mut self.dma, skip_drawing, 3);
            self.timers.step(&mut self.irq, &mut self.audio, 3);
            if let Some(do_transfer) = self.dma.step(&mut self.irq, &mut self.cart, 3) {
                do_transfer(&mut bus!(self));
            }
            self.audio.step(audio_cb, &mut self.dma, 3);
        }

        self.irq.step(&mut self.cpu, &mut self.haltcnt);
    }
}

pub struct Bus<'a, 'b, 'c> {
    pub irq: &'a mut Irq,
    pub haltcnt: &'a mut HaltControl,
    pub timers: &'a mut Timers,
    pub dma: &'a mut Dma,
    pub iwram: &'a mut [u8],
    pub ewram: &'a mut [u8],
    pub video: &'a mut Video,
    pub audio: &'a mut Audio,
    pub keypad: &'a mut Keypad,
    pub bios: &'a mut Bios<'b>,
    pub cart: &'a mut Cartridge<'c>,
    pub io_todo: &'a mut Box<[u8]>,
}

// A member fn would be nicer, but using &mut self over $gba unnecessarily mutably borrows the
// *whole* Gba struct.
#[macro_export]
macro_rules! bus {
    ($gba:ident) => {{
        $crate::gba::Bus {
            irq: &mut $gba.irq,
            haltcnt: &mut $gba.haltcnt,
            timers: &mut $gba.timers,
            dma: &mut $gba.dma,
            iwram: &mut $gba.iwram,
            ewram: &mut $gba.ewram,
            video: &mut $gba.video,
            audio: &mut $gba.audio,
            keypad: &mut $gba.keypad,
            cart: &mut $gba.cart,
            bios: &mut $gba.bios,
            io_todo: &mut $gba.io_todo,
        }
    }};
}

impl bus::Bus for Bus<'_, '_, '_> {
    fn read_byte(&mut self, addr: u32) -> u8 {
        match addr {
            // BIOS
            0x0000_0000..=0x0000_3fff => self.bios.read_byte(addr),
            // External WRAM
            0x0200_0000..=0x02ff_ffff => self.ewram.read_byte(addr & 0x3_ffff),
            // Internal WRAM
            0x0300_0000..=0x03ff_ffff => self.iwram.read_byte(addr & 0x7fff),
            // I/O Registers
            0x0400_0000..=0x0400_03fe => {
                let addr = addr & 0x3ff;
                match addr {
                    0x000..=0x056 => self.video.read_byte(addr),
                    0x060..=0x0a7 => self.audio.read_byte(addr),
                    0x0b0..=0x0df => self.dma.read_byte(addr),
                    0x100..=0x10f => self.timers.read_byte(addr),
                    0x130..=0x133 => self.keypad.read_byte(addr),
                    0x200..=0x203 | 0x208..=0x20b => self.irq.read_byte(addr),
                    0x301 => self.haltcnt.read_byte(addr),
                    0x000..=0x800 => self.io_todo[addr as usize], // TODO
                    _ => 0,
                }
            }
            // Palette RAM
            0x0500_0000..=0x05ff_ffff => self.video.palette_ram.read_byte(addr & 0x3ff),
            // VRAM
            0x0600_0000..=0x06ff_ffff => self.video.vram().read_byte(addr & 0x1_ffff),
            // OAM
            0x0700_0000..=0x07ff_ffff => self.video.oam.read_byte(addr & 0x3ff),
            // Cartridge
            0x0800_0000..=0x0fff_ffff => self.cart.read_byte(addr & 0x7ff_ffff),
            // Unused
            _ => 0xff,
        }
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        match addr {
            // External WRAM
            0x0200_0000..=0x02ff_ffff => self.ewram.write_byte(addr & 0x3_ffff, value),
            // Internal WRAM
            0x0300_0000..=0x03ff_ffff => self.iwram.write_byte(addr & 0x7fff, value),
            // I/O Registers
            0x0400_0000..=0x0400_03fe => {
                let addr = addr & 0x3ff;
                match addr {
                    0x000..=0x056 => self.video.write_byte(addr, value),
                    0x060..=0x0a7 => self.audio.write_byte(addr, value),
                    0x0b0..=0x0df => self.dma.write_byte(addr, value),
                    0x100..=0x10f => self.timers.write_byte(addr, value),
                    0x130..=0x133 => self.keypad.write_byte(addr, value),
                    0x200..=0x203 | 0x208..=0x20b => self.irq.write_byte(addr, value),
                    0x301 => self.haltcnt.write_byte(addr, value),
                    0x000..=0x800 => self.io_todo[addr as usize] = value, // TODO
                    _ => {}
                }
            }
            // Palette RAM
            0x0500_0000..=0x05ff_ffff => self.video.palette_ram.write_byte(addr & 0x3ff, value),
            // VRAM
            0x0600_0000..=0x06ff_ffff => {
                self.video.vram().write_byte(addr & 0x1_ffff, value);
            }
            // Cartridge
            0x0800_0000..=0x0fff_ffff => self.cart.write_byte(addr & 0x7ff_ffff, value),
            // Read-only, Unused, Ignored 8-bit writes to OAM/VRAM
            _ => {}
        }
    }

    fn write_hword(&mut self, addr: u32, value: u16) {
        // Video memory has weird behaviour when writing 8-bit values, so we can't simply delegate
        // such writes to write_hword_as_bytes.
        match addr {
            // Palette RAM
            0x0500_0000..=0x05ff_ffff => self.video.palette_ram.write_hword(addr & 0x3ff, value),
            // VRAM
            0x0600_0000..=0x06ff_ffff => self.video.vram().write_hword(addr & 0x1_ffff, value),
            // OAM
            0x0700_0000..=0x07ff_ffff => self.video.oam.write_hword(addr & 0x3ff, value),
            _ => bus::write_hword_as_bytes(self, addr, value),
        }
    }

    fn prefetch_instr(&mut self, addr: u32) {
        self.bios.update_protection((addr < 0x4000).then_some(addr));
    }
}
