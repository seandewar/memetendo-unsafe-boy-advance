mod isa;
pub mod reg;

use std::mem::take;

use intbits::Bits;
use log::trace;
use strum::EnumCount;
use strum_macros::{EnumCount, EnumIter, FromRepr};

use crate::bus::Bus;

use self::reg::{OperationMode, OperationState, Registers, LR_INDEX, PC_INDEX, SP_INDEX};

/// 280,896 cycles per frame at ~59.737 Hz.
pub const CYCLES_PER_SECOND: u32 = 16_779_884;

#[derive(Copy, Clone, PartialEq, Eq, FromRepr, EnumIter, EnumCount, Debug)]
pub enum Exception {
    Reset,
    DataAbort,
    FastInterrupt,
    Interrupt,
    PrefetchAbort,
    SoftwareInterrupt,
    UndefinedInstr,
}

impl Exception {
    #[must_use]
    pub fn from_priority(priority: usize) -> Option<Self> {
        Self::from_repr(priority)
    }

    #[must_use]
    pub fn priority(self) -> usize {
        self as _
    }

    #[must_use]
    pub fn vector_addr(self) -> u32 {
        match self {
            Self::Reset => 0x00,
            Self::UndefinedInstr => 0x04,
            Self::SoftwareInterrupt => 0x08,
            Self::PrefetchAbort => 0x0c,
            Self::DataAbort => 0x10,
            Self::Interrupt => 0x18,
            Self::FastInterrupt => 0x1c,
        }
    }

    #[must_use]
    pub fn return_addr_offset(self, state: OperationState) -> u32 {
        let (arm_offset, thumb_offset) = match self {
            Self::Reset => (8, 4),
            Self::DataAbort => (8, 8),
            Self::FastInterrupt | Self::Interrupt | Self::PrefetchAbort => (4, 4),
            Self::SoftwareInterrupt | Self::UndefinedInstr => (4, 2),
        };

        match state {
            OperationState::Arm => arm_offset,
            OperationState::Thumb => thumb_offset,
        }
    }

    #[must_use]
    pub fn entry_mode(self) -> OperationMode {
        match self {
            Self::Reset | Self::SoftwareInterrupt => OperationMode::Supervisor,
            Self::PrefetchAbort | Self::DataAbort => OperationMode::Abort,
            Self::Interrupt => OperationMode::Interrupt,
            Self::FastInterrupt => OperationMode::FastInterrupt,
            Self::UndefinedInstr => OperationMode::UndefinedInstr,
        }
    }

    #[must_use]
    pub fn disables_fiq(self) -> bool {
        self == Self::Reset || self == Self::FastInterrupt
    }
}

#[derive(Default, Copy, Clone, Debug)]
pub struct Cpu {
    pub reg: Registers,
    pipeline_instrs: [u32; 2],
    pipeline_reloaded: bool,
    pending_exceptions: [bool; Exception::COUNT],
}

impl Cpu {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self, bus: &mut impl Bus, skip_bios: bool) {
        self.pending_exceptions.fill(false);
        self.enter_exception(bus, Exception::Reset);

        if skip_bios {
            self.reg.r[..=12].fill(0);
            self.reg.cpsr.irq_disabled = false;
            self.reg.cpsr.fiq_disabled = false;

            self.reg.change_mode(OperationMode::Supervisor);
            self.reg.r[SP_INDEX] = 0x0300_7fe0;
            self.reg.r[LR_INDEX] = 0;
            self.reg.set_spsr(0);

            self.reg.change_mode(OperationMode::Interrupt);
            self.reg.r[SP_INDEX] = 0x0300_7fa0;
            self.reg.r[LR_INDEX] = 0;
            self.reg.set_spsr(0);

            self.reg.change_mode(OperationMode::System);
            self.reg.r[SP_INDEX] = 0x0300_7f00;
            self.reg.r[PC_INDEX] = 0x0800_0000;
            self.reload_pipeline(bus);
        }
    }

    // We only panic if the priority number of a pending exception does not map to an exception,
    // which should be impossible.
    #[expect(clippy::missing_panics_doc)]
    pub fn step(&mut self, bus: &mut impl Bus) {
        for priority in 0..self.pending_exceptions.len() {
            let raised = take(&mut self.pending_exceptions[priority]);
            let exception = Exception::from_priority(priority).unwrap();
            if raised && self.enter_exception(bus, exception) {
                return; // We serviced this exception.
            }
        }

        // NOTE: emulated pipelining will have the PC 2 instructions ahead of this executing
        // instruction, so the actual address of this instruction was PC -4 or -8.
        // The following two instructions should already be prefetched at this point.
        let instr = self.pipeline_instrs[0];
        self.pipeline_instrs[0] = self.pipeline_instrs[1];
        self.pipeline_instrs[1] = self.prefetch_instr(bus);
        self.pipeline_reloaded = false;

        trace!("next instr: {instr:08x}\n{}", self.reg);
        match self.reg.cpsr.state {
            OperationState::Arm => self.execute_arm(bus, instr),
            OperationState::Thumb => {
                self.execute_thumb(bus, instr.bits(..16).try_into().unwrap());
            }
        }
        if !self.pipeline_reloaded {
            self.reg.align_pc();
            self.reg.advance_pc();
        }
    }

    fn prefetch_instr(&mut self, bus: &mut impl Bus) -> u32 {
        bus.prefetch_instr(self.reg.r[PC_INDEX]);

        match self.reg.cpsr.state {
            OperationState::Thumb => bus.read_hword(self.reg.r[PC_INDEX]).into(),
            OperationState::Arm => bus.read_word(self.reg.r[PC_INDEX]),
        }
    }

    pub fn reload_pipeline(&mut self, bus: &mut impl Bus) {
        self.reg.align_pc();
        self.pipeline_instrs[0] = self.prefetch_instr(bus);
        self.reg.advance_pc();
        self.pipeline_instrs[1] = self.prefetch_instr(bus);
        self.reg.advance_pc();
        self.pipeline_reloaded = true;
    }

    pub fn raise_exception(&mut self, exception: Exception) {
        self.pending_exceptions[exception.priority()] = true;
    }

    fn enter_exception(&mut self, bus: &mut impl Bus, exception: Exception) -> bool {
        if (self.reg.cpsr.irq_disabled && exception == Exception::Interrupt)
            || (self.reg.cpsr.fiq_disabled && exception == Exception::FastInterrupt)
        {
            return false;
        }

        trace!("entering exception: {:?}", exception);
        let old_cpsr = self.reg.cpsr;
        self.reg.change_mode(exception.entry_mode());
        self.reg.cpsr.fiq_disabled |= exception.disables_fiq();
        self.reg.cpsr.irq_disabled = true;
        self.reg.cpsr.state = OperationState::Arm;

        let base_pc = self.reg.r[PC_INDEX].wrapping_sub(2 * old_cpsr.state.instr_size());
        self.reg.r[LR_INDEX] = base_pc.wrapping_add(exception.return_addr_offset(old_cpsr.state));
        self.reg.set_spsr(old_cpsr.bits());

        self.reg.r[PC_INDEX] = exception.vector_addr();
        self.reload_pipeline(bus);

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::bus::tests::{NullBus, VecBus};

    use strum::IntoEnumIterator;

    fn assert_exception_result(cpu: &mut Cpu, exception: Exception, old_reg: Registers) {
        assert_eq!(cpu.reg.cpsr.mode(), exception.entry_mode());
        assert_eq!(
            cpu.reg.cpsr.fiq_disabled,
            exception.disables_fiq() || old_reg.cpsr.fiq_disabled,
        );
        assert!(cpu.reg.cpsr.irq_disabled);

        // PC is offset +8 due to pipelining in the ARM state.
        assert_eq!(cpu.reg.cpsr.state, OperationState::Arm);
        assert_eq!(cpu.reg.r[PC_INDEX], exception.vector_addr().wrapping_add(8));

        // Values except PC and CPSR are technically indeterminate after a reset.
        if exception != Exception::Reset {
            let old_pc_base = old_reg.r[PC_INDEX].wrapping_sub(2 * old_reg.cpsr.state.instr_size());
            assert_eq!(
                cpu.reg.r[LR_INDEX],
                old_pc_base.wrapping_add(exception.return_addr_offset(old_reg.cpsr.state))
            );
            assert_eq!(cpu.reg.spsr(), old_reg.cpsr.bits());
        }
    }

    #[test]
    fn reset_works() {
        let mut cpu = Cpu::new();

        cpu.reg.change_mode(OperationMode::Abort);
        cpu.reg.cpsr.signed = true;
        cpu.reg.cpsr.carry = true;
        cpu.reg.cpsr.zero = false;
        cpu.reg.cpsr.overflow = false;
        cpu.reg.r[PC_INDEX] = 0xbeef;

        let old_reg = cpu.reg;
        cpu.reset(&mut NullBus, false);
        assert_exception_result(&mut cpu, Exception::Reset, old_reg);

        // condition flags should be preserved by reset
        assert!(cpu.reg.cpsr.signed);
        assert!(cpu.reg.cpsr.carry);
        assert!(!cpu.reg.cpsr.zero);
        assert!(!cpu.reg.cpsr.overflow);
    }

    #[test]
    fn enter_exception_works() {
        let mut cpu = Cpu::new();
        cpu.reset(&mut NullBus, false);
        for exception in Exception::iter() {
            cpu.reg.cpsr.fiq_disabled = false;
            cpu.reg.cpsr.irq_disabled = false;

            let old_reg = cpu.reg;
            cpu.enter_exception(&mut NullBus, exception);
            assert_exception_result(&mut cpu, exception, old_reg);
        }
    }

    #[test]
    fn raise_exception_works() {
        let mut cpu = Cpu::new();
        cpu.reset(&mut NullBus, false);

        // IRQs are also disabled on reset, so we expect the Interrupt exception to be ignored.
        cpu.reg.cpsr.fiq_disabled = false;
        cpu.raise_exception(Exception::Interrupt);
        cpu.raise_exception(Exception::SoftwareInterrupt);
        cpu.raise_exception(Exception::DataAbort);
        cpu.raise_exception(Exception::FastInterrupt);

        let assert_exception = |cpu: &mut Cpu, exception| {
            let old_reg = cpu.reg;
            cpu.step(&mut NullBus);
            assert_exception_result(cpu, exception, old_reg);
        };
        let assert_no_pending_exceptions = |cpu: &Cpu| {
            assert_eq!(cpu.pending_exceptions, [false; 7]);
        };

        assert_exception(&mut cpu, Exception::DataAbort);
        assert!(!cpu.reg.cpsr.fiq_disabled);
        assert_exception(&mut cpu, Exception::FastInterrupt);
        assert!(cpu.reg.cpsr.fiq_disabled);
        assert_exception(&mut cpu, Exception::SoftwareInterrupt);
        assert_no_pending_exceptions(&cpu);

        cpu.reg.cpsr.fiq_disabled = false;
        cpu.reg.cpsr.irq_disabled = false;
        cpu.raise_exception(Exception::UndefinedInstr);
        cpu.raise_exception(Exception::FastInterrupt);
        cpu.raise_exception(Exception::Reset); // Disables FIQ, IRQ.
        cpu.raise_exception(Exception::Interrupt);

        assert_exception(&mut cpu, Exception::Reset);
        assert_exception(&mut cpu, Exception::UndefinedInstr);
        assert_no_pending_exceptions(&cpu);

        for exception in Exception::iter() {
            cpu.raise_exception(exception);
        }

        assert_exception(&mut cpu, Exception::Reset);
        assert_exception(&mut cpu, Exception::DataAbort);
        cpu.reg.cpsr.fiq_disabled = false;
        assert_exception(&mut cpu, Exception::FastInterrupt);
        cpu.reg.cpsr.irq_disabled = false;
        assert_exception(&mut cpu, Exception::Interrupt);
        assert_exception(&mut cpu, Exception::PrefetchAbort);
        assert_exception(&mut cpu, Exception::SoftwareInterrupt);
        assert_exception(&mut cpu, Exception::UndefinedInstr);
        assert_no_pending_exceptions(&cpu);
    }

    #[expect(clippy::unusual_byte_groupings)]
    #[test]
    fn step_works() {
        let mut bus = VecBus::new(110);
        bus.write_word(0, 0b1110_00_1_1101_0_0000_0000_0000_00001001); // MOVAL R0,#(8 OR 1)
        bus.write_word(4, 0b1110_00010010111111111111_0001_0000); // BXAL R0
        bus.write_hword(8, 0b001_00_101_01100101); // MOV R5,#101
        bus.write_hword(10, 0b010001_11_0_0_101_000); // BX R5
        bus.write_hword(100, 0b001_00_001_00100001); // MOV R1,#33

        let mut cpu = Cpu::new();
        cpu.reset(&mut bus, false);
        assert_eq!(8, cpu.reg.r[PC_INDEX]);
        assert_eq!(OperationState::Arm, cpu.reg.cpsr.state);

        cpu.step(&mut bus);
        assert_eq!(4 + 8, cpu.reg.r[PC_INDEX]);
        assert_eq!(8 | 1, cpu.reg.r[0]);

        cpu.step(&mut bus);
        assert_eq!(8 + 4, cpu.reg.r[PC_INDEX]);
        assert_eq!(OperationState::Thumb, cpu.reg.cpsr.state);

        cpu.step(&mut bus);
        assert_eq!(10 + 4, cpu.reg.r[PC_INDEX]);
        assert_eq!(101, cpu.reg.r[5]);

        cpu.step(&mut bus);
        assert_eq!(100 + 4, cpu.reg.r[PC_INDEX]);
        assert_eq!(OperationState::Thumb, cpu.reg.cpsr.state);

        cpu.step(&mut bus);
        assert_eq!(102 + 4, cpu.reg.r[PC_INDEX]);
        assert_eq!(33, cpu.reg.r[1]);
    }
}
