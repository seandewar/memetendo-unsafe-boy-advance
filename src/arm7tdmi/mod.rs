mod op;
mod reg;
mod thumb;

use self::reg::{OperationMode, OperationState, Registers, LR_INDEX, PC_INDEX};

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
    #[allow(unused)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self, bus: &impl DataBus) {
        self.run_state = RunState::Running;
        self.enter_exception(bus, Exception::Reset);
    }

    pub fn step(&mut self, bus: &mut impl DataBus) {
        if self.run_state != RunState::Running {
            return;
        }

        let instr = self.flush_pipeline(bus);
        match self.reg.cpsr.state {
            OperationState::Arm => {
                // TODO: ARM instruction set is unimplemented rn so just do THUMB
                self.execute_bx(bus, self.reg.r[PC_INDEX].wrapping_sub(8));
            }
            OperationState::Thumb => {
                self.execute_thumb(bus, (instr & 0xffff) as _);
            }
        }
    }

    fn flush_pipeline(&mut self, bus: &impl DataBus) -> u32 {
        let instr_size = self.reg.cpsr.state.instr_size();
        let instr = self.pipeline_instrs[0];

        self.pipeline_instrs[0] = self.pipeline_instrs[1];
        self.pipeline_instrs[1] = match self.reg.cpsr.state {
            OperationState::Thumb => bus.read_hword(self.reg.r[PC_INDEX]).into(),
            OperationState::Arm => bus.read_word(self.reg.r[PC_INDEX]),
        };
        self.reg.r[PC_INDEX] = self.reg.r[PC_INDEX].wrapping_add(instr_size);

        instr
    }

    /// NOTE: also aligns PC.
    fn reload_pipeline(&mut self, bus: &impl DataBus) {
        let instr_size = self.reg.cpsr.state.instr_size();

        self.reg.r[PC_INDEX] = match self.reg.cpsr.state {
            OperationState::Thumb => {
                let pc = self.reg.r[PC_INDEX] & !1;
                self.pipeline_instrs[0] = bus.read_hword(pc).into();
                self.pipeline_instrs[1] = bus.read_hword(pc.wrapping_add(instr_size)).into();

                pc
            }
            OperationState::Arm => {
                let pc = self.reg.r[PC_INDEX] & !0b11;
                self.pipeline_instrs[0] = bus.read_word(pc);
                self.pipeline_instrs[1] = bus.read_word(pc.wrapping_add(instr_size));

                pc
            }
        }
        .wrapping_add(instr_size * 2);
    }

    fn set_cpsr(&mut self, cpsr: u32) {
        if self.reg.set_cpsr(cpsr).is_err() {
            self.run_state = RunState::Hung;
        }
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
        self.reg.r[LR_INDEX] = self.reg.r[PC_INDEX];
        self.reg.r[PC_INDEX] = exception.vector_addr();

        self.reload_pipeline(bus);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::bus::{NullBus, VecBus};

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

        // +8 in PC due to pipe-lining
        assert_eq!(exception.vector_addr().wrapping_add(8), cpu.reg.r[PC_INDEX]);
        assert_eq!(old_reg.r[PC_INDEX], cpu.reg.r[LR_INDEX]);
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
        cpu.reg.r[PC_INDEX] = 0xbeef;
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

    #[allow(clippy::unusual_byte_groupings)]
    #[test]
    fn step_works() {
        let mut bus = VecBus(vec![0; 102]);
        bus.write_hword(0, 0b001_00_101_01100100); // MOV R5,#100
        bus.write_hword(2, 0b010001_11_0_0_101_000); // BX R5
        bus.write_hword(100, 0b001_00_001_00100001); // MOV R1,#33

        let mut cpu = Cpu::new();
        cpu.reset(&bus);
        cpu.execute_bx(&bus, 0); // act like the CPU started in THUMB mode
        assert_eq!(4, cpu.reg.r[PC_INDEX]);
        assert_eq!(OperationState::Thumb, cpu.reg.cpsr.state);

        cpu.step(&mut bus);
        assert_eq!(6, cpu.reg.r[PC_INDEX]);
        assert_eq!(100, cpu.reg.r[5]);

        cpu.step(&mut bus);
        assert_eq!(104, cpu.reg.r[PC_INDEX]);
        assert_eq!(OperationState::Thumb, cpu.reg.cpsr.state);

        cpu.step(&mut bus);
        assert_eq!(106, cpu.reg.r[PC_INDEX]);
        assert_eq!(33, cpu.reg.r[1]);
    }
}
