mod op;
mod reg;
mod thumb;

use self::reg::{
    NamedGeneralRegister::{Lr, Pc},
    OperationMode, OperationState, Registers,
};

use strum_macros::EnumIter;

use crate::bus::DataBus;

#[derive(Default, Debug)]
pub struct Cpu {
    run_state: RunState,
    reg: Registers,
    pipeline_instrs: [u32; 2],
}

#[derive(PartialEq, Eq, Debug)]
enum RunState {
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
        Self::default()
    }

    pub fn reset(&mut self, bus: &impl DataBus) {
        self.run_state = RunState::Running;
        self.enter_exception(bus, Exception::Reset);
    }

    pub fn step(&mut self, _bus: &mut impl DataBus) {
        if self.run_state != RunState::Running {
            return;
        }
        todo!();
    }

    fn set_cpsr(&mut self, cpsr: u32) {
        if self.reg.set_cpsr(cpsr).is_err() {
            self.run_state = RunState::Hung;
        }
    }

    fn reload_pipeline(&mut self, bus: &impl DataBus) {
        let pc = self.reg.r[Pc];
        let pc_offset = match self.reg.cpsr.state {
            OperationState::Thumb => {
                self.pipeline_instrs[0] = bus.read_hword(pc).into();
                self.pipeline_instrs[1] = bus.read_hword(pc.wrapping_add(2)).into();
                4
            }
            OperationState::Arm => {
                self.pipeline_instrs[0] = bus.read_word(pc);
                self.pipeline_instrs[1] = bus.read_word(pc.wrapping_add(4));
                8
            }
        };

        self.reg.r[Pc] = pc.wrapping_add(pc_offset);
    }
}

#[derive(Copy, Clone, PartialEq, Eq, EnumIter, Debug)]
#[repr(u32)]
enum Exception {
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
    fn vector_addr(self) -> u32 {
        self as _
    }

    #[must_use]
    fn entry_mode(self) -> OperationMode {
        match self {
            Self::Reset | Self::SoftwareInterrupt => OperationMode::Supervisor,
            Self::PrefetchAbort | Self::DataAbort => OperationMode::Abort,
            Self::Interrupt => OperationMode::Interrupt,
            Self::FastInterrupt => OperationMode::FastInterrupt,
            Self::UndefinedInstr => OperationMode::UndefinedInstr,
        }
    }

    #[must_use]
    fn disables_fiq(self) -> bool {
        matches!(self, Self::Reset | Self::FastInterrupt)
    }
}

impl Cpu {
    fn enter_exception(&mut self, bus: &impl DataBus, exception: Exception) {
        let old_cpsr = self.reg.cpsr;

        self.reg.set_mode(exception.entry_mode());
        self.reg.cpsr.fiq_disabled |= exception.disables_fiq();
        self.reg.cpsr.irq_disabled = true;
        self.reg.cpsr.state = OperationState::Arm;

        self.reg.spsr = old_cpsr;
        self.reg.r[Lr] = self.reg.r[Pc];
        self.reg.r[Pc] = exception.vector_addr();

        self.reload_pipeline(bus);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::bus::NullBus;

    use strum::IntoEnumIterator;

    #[test]
    fn set_cpsr_works() {
        let mut cpu = Cpu::new();
        cpu.reset(&NullBus);

        cpu.set_cpsr(OperationMode::Abort.psr() | (1 << 5));
        assert_eq!(RunState::Running, cpu.run_state);
        assert_eq!(OperationMode::Abort, cpu.reg.cpsr.mode());
        assert_eq!(OperationState::Thumb, cpu.reg.cpsr.state);

        cpu.set_cpsr(OperationMode::UndefinedInstr.psr());
        assert_eq!(RunState::Running, cpu.run_state);
        assert_eq!(OperationMode::UndefinedInstr, cpu.reg.cpsr.mode());
        assert_eq!(OperationState::Arm, cpu.reg.cpsr.state);

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

        // +8 in PC due to pipelining
        assert_eq!(exception.vector_addr().wrapping_add(8), cpu.reg.r[Pc]);
        assert_eq!(old_reg.r[Pc], cpu.reg.r[Lr]);
        assert_eq!(old_reg.cpsr, cpu.reg.spsr);
        assert_eq!(OperationState::Arm, cpu.reg.cpsr.state);
        assert!(cpu.reg.cpsr.irq_disabled);
    }

    fn test_exception(cpu: &mut Cpu, exception: Exception) {
        let old_reg = cpu.reg;
        cpu.enter_exception(&NullBus, exception);
        assert_exception_result(cpu, exception, old_reg);
    }

    #[test]
    fn reset_works() {
        let mut cpu = Cpu::new();
        cpu.set_cpsr((0b1111 << 28) | 0b1111_1111);
        cpu.reg.r[Pc] = 0xbeef;
        let old_reg = cpu.reg;

        cpu.reset(&NullBus);
        assert_exception_result(&mut cpu, Exception::Reset, old_reg);

        // condition flags should be preserved by reset
        assert!(cpu.reg.cpsr.negative);
        assert!(cpu.reg.cpsr.zero);
        assert!(cpu.reg.cpsr.carry);
        assert!(cpu.reg.cpsr.overflow);
    }

    #[test]
    fn enter_exception_works() {
        let mut cpu = Cpu::new();
        cpu.reset(&NullBus);
        for exception in Exception::iter() {
            test_exception(&mut cpu, exception);
        }
    }
}
