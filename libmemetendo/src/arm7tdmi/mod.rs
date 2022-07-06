mod isa;
pub mod reg;

use std::{
    error,
    fmt::{self, Display, Formatter},
    mem::replace,
    result,
};

use strum_macros::{EnumIter, FromRepr};

use crate::bus::Bus;

use self::reg::{OperationMode, OperationState, Registers, LR_INDEX, PC_INDEX, SP_INDEX};

#[derive(Copy, Clone, PartialEq, Eq, FromRepr, EnumIter, Debug)]
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

#[derive(Default, Copy, Clone, PartialEq, Eq, Debug)]
pub enum RunState {
    #[default]
    NotRunning,
    Running,
    Errored(Error),
}

#[derive(Default, Debug)]
pub struct Cpu {
    state: RunState,
    pub reg: Registers,
    pipeline_instrs: [u32; 2],
    pending_exceptions: [bool; 7],
}

impl Cpu {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self, bus: &mut impl Bus, skip_bios: bool) {
        self.state = RunState::Running;
        self.pending_exceptions.fill(false);

        self.enter_exception(bus, Exception::Reset);
        self.step_pipeline(bus);

        if skip_bios {
            self.reg.r[..=12].fill(0);
            self.reg.cpsr.irq_disabled = false;
            self.reg.cpsr.fiq_disabled = false;

            self.reg.change_mode(OperationMode::Supervisor);
            self.reg.r[SP_INDEX] = 0x0300_7fe0;
            self.reg.r[LR_INDEX] = 0;
            self.reg.spsr = 0;

            self.reg.change_mode(OperationMode::Interrupt);
            self.reg.r[SP_INDEX] = 0x0300_7fa0;
            self.reg.r[LR_INDEX] = 0;
            self.reg.spsr = 0;

            self.reg.change_mode(OperationMode::System);
            self.reg.r[SP_INDEX] = 0x0300_7f00;
            self.reg.r[PC_INDEX] = 0x0800_0000;
            self.reload_pipeline(bus);
            self.step_pipeline(bus);
        }
    }

    // We only panic if the priority number of a pending exception does not map to an exception,
    // which should be impossible.
    #[allow(clippy::missing_panics_doc)]
    /// # Errors
    ///
    /// Returns an error if some sort of invalid or unhandled operation occurred.
    /// See [`Error`] for a list of possibilities.
    pub fn step(&mut self, bus: &mut impl Bus) -> Result<()> {
        if self.state != RunState::Running {
            return Ok(());
        }

        for priority in 0..self.pending_exceptions.len() {
            let raised = replace(&mut self.pending_exceptions[priority], false);
            let exception = Exception::from_priority(priority).unwrap();
            if raised && self.enter_exception(bus, exception) {
                // We serviced this exception.
                self.step_pipeline(bus);
                return Ok(());
            }
        }

        // let regs = self
        //     .reg
        //     .r
        //     .iter()
        //     .copied()
        //     .map(|x| format!("{x:0x}"))
        //     .collect::<Vec<_>>()
        //     .join(", ");
        // println!(
        //     "{:08x}: {:08x}, r: [{regs}], cpsr: {:08x}, spsr {:08x}",
        //     self.reg.r[PC_INDEX],
        //     self.pipeline_instrs[0],
        //     self.reg.cpsr.bits(),
        //     self.reg.spsr
        // );

        let instr = self.pipeline_instrs[0];
        let result = match self.reg.cpsr.state {
            OperationState::Arm => self.execute_arm(bus, instr),
            OperationState::Thumb =>
            {
                #[allow(clippy::cast_possible_truncation)]
                self.execute_thumb(bus, instr as u16)
            }
        };
        if let Err(error) = result {
            self.state = RunState::Errored(error);
        } else {
            self.step_pipeline(bus);
        }

        result
    }

    pub fn step_pipeline(&mut self, bus: &mut impl Bus) {
        self.reg.align_pc();
        self.pipeline_instrs[0] = self.pipeline_instrs[1];
        self.pipeline_instrs[1] = match self.reg.cpsr.state {
            OperationState::Thumb => bus.read_hword(self.reg.r[PC_INDEX]).into(),
            OperationState::Arm => bus.read_word(self.reg.r[PC_INDEX]),
        };

        let instr_size = self.reg.cpsr.state.instr_size();
        self.reg.r[PC_INDEX] = self.reg.r[PC_INDEX].wrapping_add(instr_size);
        bus.prefetch_instr(self.reg.r[PC_INDEX]);
    }

    /// Forcibly aligns the PC and flushes the instruction pipeline, then fetches the next
    /// instruction at the PC, then advances the PC by one instruction.
    pub fn reload_pipeline(&mut self, bus: &mut impl Bus) {
        self.reg.align_pc();
        bus.prefetch_instr(self.reg.r[PC_INDEX]);
        self.step_pipeline(bus);
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

        let old_cpsr = self.reg.cpsr;
        self.reg.change_mode(exception.entry_mode());
        self.reg.cpsr.fiq_disabled |= exception.disables_fiq();
        self.reg.cpsr.irq_disabled = true;
        self.reg.cpsr.state = OperationState::Arm;

        let base_pc = self.reg.r[PC_INDEX].wrapping_sub(2 * old_cpsr.state.instr_size());
        self.reg.r[LR_INDEX] = base_pc.wrapping_add(exception.return_addr_offset(old_cpsr.state));
        self.reg.spsr = old_cpsr.bits();

        self.reg.r[PC_INDEX] = exception.vector_addr();
        self.reload_pipeline(bus);

        true
    }
}

pub type Result<T> = result::Result<T, Error>;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Error {
    InvalidOperationMode(u8),
    MsrChangedOperationState(OperationState),
}

impl error::Error for Error {}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let message = match self {
            Error::InvalidOperationMode(bits) => format!("Invalid operation mode ({bits:#07b})"),
            Error::MsrChangedOperationState(state) => {
                format!("MSR instruction changed the operation state ({state:?})")
            }
        };

        f.write_str(&message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::bus::tests::{NullBus, VecBus};

    use strum::IntoEnumIterator;

    fn assert_exception_result(cpu: &mut Cpu, exception: Exception, old_reg: Registers) {
        assert_eq!(cpu.state, RunState::Running);
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
            assert_eq!(cpu.reg.spsr, old_reg.cpsr.bits());
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
            cpu.step_pipeline(&mut NullBus);
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
            cpu.step(&mut NullBus).unwrap();
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

    #[allow(clippy::unusual_byte_groupings)]
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

        cpu.step(&mut bus).unwrap();
        assert_eq!(4 + 8, cpu.reg.r[PC_INDEX]);
        assert_eq!(8 | 1, cpu.reg.r[0]);

        cpu.step(&mut bus).unwrap();
        assert_eq!(8 + 4, cpu.reg.r[PC_INDEX]);
        assert_eq!(OperationState::Thumb, cpu.reg.cpsr.state);

        cpu.step(&mut bus).unwrap();
        assert_eq!(10 + 4, cpu.reg.r[PC_INDEX]);
        assert_eq!(101, cpu.reg.r[5]);

        cpu.step(&mut bus).unwrap();
        assert_eq!(100 + 4, cpu.reg.r[PC_INDEX]);
        assert_eq!(OperationState::Thumb, cpu.reg.cpsr.state);

        cpu.step(&mut bus).unwrap();
        assert_eq!(102 + 4, cpu.reg.r[PC_INDEX]);
        assert_eq!(33, cpu.reg.r[1]);
    }
}
