mod reg;
mod thumb;

use self::reg::{NamedGeneralRegister::*, OperationMode, Registers};

use crate::bus::DataBus;

use strum_macros::EnumIter;

#[derive(Default, Debug)]
pub struct Cpu {
    run_state: RunState,
    reg: Registers,
}

#[derive(PartialEq, Eq, Debug)]
pub enum RunState {
    NotRunning,
    Running,
    Hung,
}

impl Default for RunState {
    fn default() -> Self {
        Self::NotRunning
    }
}

impl Cpu {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn reset(&mut self) {
        self.run_state = RunState::Running;
        self.enter_exception(Exception::Reset);
    }

    pub(crate) fn set_cpsr(&mut self, cpsr: u32) {
        if self.reg.set_cpsr(cpsr).is_err() {
            self.run_state = RunState::Hung;
        }
    }

    pub fn step(&mut self, bus: &mut impl DataBus, cycles: usize) {
        if self.run_state != RunState::Running {
            return;
        }
        todo!();
    }
}

#[derive(Copy, Clone, PartialEq, Eq, EnumIter, Debug)]
#[repr(u32)]
pub(crate) enum Exception {
    Reset = 0x00,
    UndefinedInstr = 0x04,
    SoftwareInterrupt = 0x08,
    PrefetchAbort = 0x0c,
    DataAbort = 0x10,
    Interrupt = 0x18,
    FastInterrupt = 0x1c,
}

impl Exception {
    #[must_use]
    fn vector_addr(&self) -> u32 {
        *self as _
    }

    #[must_use]
    fn entry_mode(&self) -> OperationMode {
        match self {
            Self::Reset => OperationMode::Supervisor,
            Self::UndefinedInstr => OperationMode::UndefinedInstr,
            Self::SoftwareInterrupt => OperationMode::Supervisor,
            Self::PrefetchAbort => OperationMode::Abort,
            Self::DataAbort => OperationMode::Abort,
            Self::Interrupt => OperationMode::Interrupt,
            Self::FastInterrupt => OperationMode::FastInterrupt,
        }
    }

    #[must_use]
    fn disables_fiq(&self) -> bool {
        matches!(self, Self::Reset | Self::FastInterrupt)
    }
}

impl Cpu {
    pub(crate) fn enter_exception(&mut self, exception: Exception) {
        let old_cpsr = self.reg.cpsr;

        self.reg.set_mode(exception.entry_mode());
        self.reg.cpsr.fiq_disabled |= exception.disables_fiq();
        self.reg.cpsr.irq_disabled = true;
        self.reg.cpsr.thumb_enabled = false;

        self.reg.spsr = old_cpsr;
        self.reg.r[Lr] = self.reg.r[Pc]; // TODO: PC+nn?
        self.reg.r[Pc] = exception.vector_addr();
    }
}

#[cfg(test)]
mod tests {
    use strum::IntoEnumIterator;

    use super::*;

    #[test]
    fn set_cpsr_works() {
        let mut cpu = Cpu::new();
        cpu.reset();

        cpu.set_cpsr(OperationMode::Abort.psr() | (1 << 5));
        assert_eq!(RunState::Running, cpu.run_state);
        assert_eq!(OperationMode::Abort, cpu.reg.cpsr.mode());
        assert!(cpu.reg.cpsr.thumb_enabled);

        cpu.set_cpsr(OperationMode::UndefinedInstr.psr());
        assert_eq!(RunState::Running, cpu.run_state);
        assert_eq!(OperationMode::UndefinedInstr, cpu.reg.cpsr.mode());
        assert!(!cpu.reg.cpsr.thumb_enabled);

        // invalid cpsr mode should hang
        cpu.set_cpsr(0);
        assert_eq!(RunState::Hung, cpu.run_state);
    }

    fn assert_exception_result(cpu: &mut Cpu, exception: Exception, old_reg: Registers) {
        assert_eq!(RunState::Running, cpu.run_state);
        assert_eq!(exception.entry_mode(), cpu.reg.cpsr.mode());
        assert_eq!(
            exception.disables_fiq() || old_reg.cpsr.fiq_disabled,
            cpu.reg.cpsr.fiq_disabled
        );
        assert_eq!(exception.vector_addr(), cpu.reg.r[Pc]);
        assert_eq!(old_reg.r[Pc], cpu.reg.r[Lr]);
        assert_eq!(old_reg.cpsr, cpu.reg.spsr);
        assert!(!cpu.reg.cpsr.thumb_enabled);
        assert!(cpu.reg.cpsr.irq_disabled);
    }

    fn test_exception(cpu: &mut Cpu, exception: Exception) {
        let old_reg = cpu.reg;
        cpu.enter_exception(exception);
        assert_exception_result(cpu, exception, old_reg);
    }

    #[test]
    fn reset_works() {
        let mut cpu = Cpu::new();
        cpu.set_cpsr((0b1111 << 28) | 0b1111_1111);
        cpu.reg.r[Pc] = 0xbeef;
        let old_reg = cpu.reg;

        cpu.reset();
        assert_exception_result(&mut cpu, Exception::Reset, old_reg);

        // condition flags should be preserved by reset
        assert!(cpu.reg.cpsr.sign);
        assert!(cpu.reg.cpsr.zero);
        assert!(cpu.reg.cpsr.carry);
        assert!(cpu.reg.cpsr.overflow);
    }

    #[test]
    fn enter_exception_works() {
        let mut cpu = Cpu::new();
        cpu.reset();
        for exception in Exception::iter() {
            test_exception(&mut cpu, exception);
        }
    }
}
