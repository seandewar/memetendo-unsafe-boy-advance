mod isa;
mod reg;

use self::reg::{OperationMode, OperationState, Registers, LR_INDEX, PC_INDEX};

use strum_macros::EnumIter;

use crate::bus::Bus;

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

    pub fn reset(&mut self, bus: &impl Bus) {
        self.run_state = RunState::Running;

        self.enter_exception(bus, Exception::Reset);
        self.step_pipeline(bus);

        // Values other than PC and CPSR are considered indeterminate after a reset.
        // enter_exception gives LR an ugly value here; set it to zero for consistency.
        self.reg.r[LR_INDEX] = 0;
    }

    pub fn step(&mut self, bus: &mut impl Bus) {
        if self.run_state != RunState::Running {
            return;
        }

        let instr = self.pipeline_instrs[0];
        match self.reg.cpsr.state {
            OperationState::Arm => self.execute_arm(bus, instr),
            OperationState::Thumb => {
                #[allow(clippy::cast_possible_truncation)]
                self.execute_thumb(bus, instr as u16);
            }
        }
        self.step_pipeline(bus);
    }

    fn step_pipeline(&mut self, bus: &impl Bus) {
        use crate::bus::BusExt; // PC is forcibly aligned anyway
        self.reg.r[PC_INDEX] &= match self.reg.cpsr.state {
            OperationState::Thumb => !1,
            OperationState::Arm => !0b11,
        };

        self.pipeline_instrs[0] = self.pipeline_instrs[1];
        self.pipeline_instrs[1] = match self.reg.cpsr.state {
            OperationState::Thumb => bus.read_hword(self.reg.r[PC_INDEX]).into(),
            OperationState::Arm => bus.read_word(self.reg.r[PC_INDEX]),
        };

        let instr_size = self.reg.cpsr.state.instr_size();
        self.reg.r[PC_INDEX] = self.reg.r[PC_INDEX].wrapping_add(instr_size);
    }

    /// Forcibly aligns the PC and flushes the instruction pipeline, then fetches the next
    /// instruction at the PC, then advances the PC by one instruction.
    ///
    /// NOTE: The next instruction in the pipeline will be 0, as it is expected that
    /// `step_pipeline()` will be called before getting the next instruction from the pipeline.
    fn reload_pipeline(&mut self, bus: &impl Bus) {
        self.pipeline_instrs[0] = 0;
        self.step_pipeline(bus);
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
    fn vector_addr(self) -> u32 {
        self as _
    }

    fn entry_mode(self) -> OperationMode {
        match self {
            Self::Reset | Self::SoftwareInterrupt => OperationMode::Supervisor,
            Self::PrefetchAbort | Self::DataAbort => OperationMode::Abort,
            Self::Interrupt => OperationMode::Interrupt,
            Self::FastInterrupt => OperationMode::FastInterrupt,
            Self::UndefinedInstr => OperationMode::UndefinedInstr,
        }
    }

    fn disables_fiq(self) -> bool {
        matches!(self, Self::Reset | Self::FastInterrupt)
    }
}

impl Cpu {
    fn enter_exception(&mut self, bus: &impl Bus, exception: Exception) {
        let old_cpsr = self.reg.cpsr;

        self.reg.change_mode(exception.entry_mode());
        self.reg.cpsr.fiq_disabled |= exception.disables_fiq();
        self.reg.cpsr.irq_disabled = true;
        self.reg.cpsr.state = OperationState::Arm;

        self.reg.spsr = old_cpsr;
        self.reg.r[LR_INDEX] = self.reg.r[PC_INDEX].wrapping_sub(self.reg.cpsr.state.instr_size());
        self.reg.r[PC_INDEX] = exception.vector_addr();
        self.reload_pipeline(bus);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::bus::{
        tests::{NullBus, VecBus},
        BusExt,
    };

    use strum::IntoEnumIterator;

    fn assert_exception_result(cpu: &mut Cpu, exception: Exception, old_reg: Registers) {
        assert_eq!(RunState::Running, cpu.run_state);
        assert_eq!(exception.entry_mode(), cpu.reg.cpsr.mode);
        assert_eq!(
            exception.disables_fiq() || old_reg.cpsr.fiq_disabled,
            cpu.reg.cpsr.fiq_disabled
        );
        assert!(cpu.reg.cpsr.irq_disabled);

        // PC is offset +8 due to pipelining in the ARM state.
        assert_eq!(OperationState::Arm, cpu.reg.cpsr.state);
        assert_eq!(exception.vector_addr().wrapping_add(8), cpu.reg.r[PC_INDEX]);

        // Values except PC and CPSR are indeterminate after a reset.
        if exception != Exception::Reset {
            assert_eq!(old_reg.r[PC_INDEX].wrapping_sub(4), cpu.reg.r[LR_INDEX]);
            assert_eq!(old_reg.cpsr, cpu.reg.spsr);
        }
    }

    fn test_exception(cpu: &mut Cpu, exception: Exception) {
        let old_reg = cpu.reg;
        cpu.enter_exception(&NullBus, exception);
        cpu.step_pipeline(&NullBus);
        assert_exception_result(cpu, exception, old_reg);
    }

    #[test]
    fn reset_works() {
        let mut cpu = Cpu::new();

        cpu.reg.change_mode(OperationMode::Abort);
        cpu.reg.cpsr.set_flags_from_bits(0b1010 << 28);
        cpu.reg.r[PC_INDEX] = 0xbeef;
        let old_reg = cpu.reg;

        cpu.reset(&NullBus);
        assert_exception_result(&mut cpu, Exception::Reset, old_reg);

        // condition flags should be preserved by reset
        assert!(cpu.reg.cpsr.signed);
        assert!(!cpu.reg.cpsr.zero);
        assert!(cpu.reg.cpsr.carry);
        assert!(!cpu.reg.cpsr.overflow);
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
        let mut bus = VecBus::new(110);
        bus.write_word(0, 0b1110_00_1_1101_0_0000_0000_0000_00001001); // MOVAL R0,#(8 OR 1)
        bus.write_word(4, 0b1110_00010010111111111111_0001_0000); // BXAL R0
        bus.write_hword(8, 0b001_00_101_01100101); // MOV R5,#101
        bus.write_hword(10, 0b010001_11_0_0_101_000); // BX R5
        bus.write_hword(100, 0b001_00_001_00100001); // MOV R1,#33

        let mut cpu = Cpu::new();
        cpu.reset(&bus);
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
