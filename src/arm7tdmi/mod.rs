mod reg;

use num_enum::IntoPrimitive;

use self::reg::{NamedGeneralRegister::*, OperationMode, Registers};

#[derive(Default, Debug)]
pub struct Cpu {
    reg: Registers,
    run_state: RunState,
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

    pub fn step(&mut self, cycles: usize) {
        if self.run_state != RunState::Running {
            return;
        }

        // TODO: cycles may need to be isize, and we'll likely need to accumulate steps if the last
        // instruction that we can execute here requires more steps than is available
        todo!()
    }
}

#[derive(PartialEq, Eq, IntoPrimitive, Debug)]
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
}

impl Cpu {
    pub(crate) fn enter_exception(&mut self, exception: Exception) {
        let old_cpsr = self.reg.cpsr;

        self.reg.set_mode(exception.entry_mode());
        self.reg.cpsr.thumb_enabled = false;
        self.reg.cpsr.irq_disabled = true;
        self.reg.cpsr.fiq_disabled =
            exception == Exception::Reset || exception == Exception::FastInterrupt;

        self.reg.spsr = old_cpsr;
        self.reg.r[Lr] = self.reg.r[Pc];
        self.reg.r[Pc] = exception.into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_cpsr_works() {
        let mut cpu = Cpu::new();
        cpu.reset();

        cpu.set_cpsr(OperationMode::Abort as u32 | (1 << 5));
        assert_eq!(RunState::Running, cpu.run_state);
        assert_eq!(OperationMode::Abort, cpu.reg.cpsr.mode());
        assert!(cpu.reg.cpsr.thumb_enabled);

        cpu.set_cpsr(OperationMode::UndefinedInstr as _);
        assert_eq!(RunState::Running, cpu.run_state);
        assert_eq!(OperationMode::UndefinedInstr, cpu.reg.cpsr.mode());
        assert!(!cpu.reg.cpsr.thumb_enabled);

        // invalid cpsr mode should hang
        cpu.set_cpsr(0);
        assert_eq!(RunState::Hung, cpu.run_state);
    }

    #[test]
    fn reset_works() {
        let mut cpu = Cpu::new();
        cpu.set_cpsr((0b1111 << 28) | 0b1111_1111);
        cpu.reg.r[Pc] = 0xbeef;
        cpu.reset();

        assert_eq!(RunState::Running, cpu.run_state);
        assert_eq!(OperationMode::Supervisor, cpu.reg.cpsr.mode());
        assert_eq!(0, cpu.reg.r[Pc]);
        assert!(!cpu.reg.cpsr.thumb_enabled);
        assert!(cpu.reg.cpsr.irq_disabled);
        assert!(cpu.reg.cpsr.fiq_disabled);

        // condition flags should be preserved
        assert!(cpu.reg.cpsr.sign);
        assert!(cpu.reg.cpsr.zero);
        assert!(cpu.reg.cpsr.carry);
        assert!(cpu.reg.cpsr.overflow);
    }
}
