#[derive(Debug)]
pub struct Cpu {
    run_state: RunState,
    op_state: OperationState,
    op_mode: OperationMode,
    reg: Registers,
}

#[derive(PartialEq, Eq, Debug)]
enum RunState {
    NotRunning,
    Running,
    Hung,
}

#[derive(PartialEq, Eq, Debug)]
enum OperationState {
    Arm,
    Thumb,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum OperationMode {
    User,
    FastInterrupt,
    Interrupt,
    Supervisor,
    Abort,
    System,
    Undefined,
}

#[derive(Default, Debug)]
struct Registers {
    r: [u32; 16],
    cpsr: u32,
    spsr: u32,
    banks: [RegisterBank; 6],
    fiq_r8_12_bank: [u32; 5],
}

#[derive(Default, PartialEq, Eq, Debug)]
struct RegisterBank {
    r13: u32,
    r14: u32,
    spsr: u32,
}

const R_SP_INDEX: usize = 13;
const R_LR_INDEX: usize = 14;
const R_PC_INDEX: usize = 15;

const PSR_NEG_LT_MASK: u32 = 1 << 31;
const PSR_ZERO_MASK: u32 = 1 << 30;
const PSR_CARRY_MASK: u32 = 1 << 29;
const PSR_OVERFLOW_MASK: u32 = 1 << 28;

const PSR_IRQ_DISABLE_MASK: u32 = 1 << 7;
const PSR_FIQ_DISABLE_MASK: u32 = 1 << 6;
const PSR_OP_THUMB_MASK: u32 = 1 << 5;
const PSR_OP_MODE_MASK: u32 = 0b11111;

impl OperationMode {
    fn bank_index(self) -> usize {
        match self {
            Self::User | Self::System => 0,
            Self::FastInterrupt => 1,
            Self::Interrupt => 2,
            Self::Supervisor => 3,
            Self::Abort => 4,
            Self::Undefined => 5,
        }
    }

    fn psr_mode_bits(self) -> u32 {
        match self {
            Self::User => 0b10000,
            Self::FastInterrupt => 0b10001,
            Self::Interrupt => 0b10010,
            Self::Supervisor => 0b10011,
            Self::Abort => 0b10111,
            Self::System => 0b11011,
            Self::Undefined => 0b11111,
        }
    }

    fn try_from_psr(psr: u32) -> Option<Self> {
        match psr & PSR_OP_MODE_MASK {
            0b10000 => Some(Self::User),
            0b10001 => Some(Self::FastInterrupt),
            0b10010 => Some(Self::Interrupt),
            0b10011 => Some(Self::Supervisor),
            0b10111 => Some(Self::Abort),
            0b11011 => Some(Self::System),
            0b11111 => Some(Self::Undefined),
            _ => None,
        }
    }
}

impl OperationState {
    fn from_psr(psr: u32) -> Self {
        if psr & PSR_OP_THUMB_MASK == 0 {
            Self::Arm
        } else {
            Self::Thumb
        }
    }
}

impl Default for Cpu {
    fn default() -> Self {
        Self {
            run_state: RunState::NotRunning,
            op_state: OperationState::Arm,
            op_mode: OperationMode::Supervisor, // value doesn't matter yet
            reg: Default::default(),
        }
    }
}

impl Cpu {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn reset(&mut self) {
        self.run_state = RunState::Running;
        let mut new_cpsr = (self.reg.cpsr & !PSR_OP_THUMB_MASK) & !PSR_OP_MODE_MASK;
        new_cpsr |=
            OperationMode::Supervisor.psr_mode_bits() | PSR_IRQ_DISABLE_MASK | PSR_FIQ_DISABLE_MASK;
        self.set_cpsr(new_cpsr);
        self.reg.r[R_PC_INDEX] = 0;
    }

    pub fn set_cpsr(&mut self, cpsr: u32) {
        self.reg.cpsr = cpsr;
        if let Some(mode) = OperationMode::try_from_psr(cpsr) {
            self.change_mode(mode);
            self.op_state = OperationState::from_psr(cpsr);
        } else {
            self.run_state = RunState::Hung;
        }
    }

    fn change_mode(&mut self, mode: OperationMode) {
        let old_mode = self.op_mode;
        let old_bank_i = old_mode.bank_index();

        self.op_mode = mode;
        let bank_i = mode.bank_index();
        if old_bank_i == bank_i {
            return;
        }

        if old_mode == OperationMode::FastInterrupt || mode == OperationMode::FastInterrupt {
            self.reg
                .fiq_r8_12_bank
                .swap_with_slice(&mut self.reg.r[8..=12]);
        }

        self.reg.banks[old_bank_i].r13 = self.reg.r[13];
        self.reg.banks[old_bank_i].r14 = self.reg.r[14];
        self.reg.banks[old_bank_i].spsr = self.reg.spsr;

        self.reg.r[13] = self.reg.banks[bank_i].r13;
        self.reg.r[14] = self.reg.banks[bank_i].r14;
        self.reg.spsr = self.reg.banks[bank_i].spsr;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bank_index_usr_and_sys_mode_same_index() {
        assert_eq!(
            OperationMode::User.bank_index(),
            OperationMode::System.bank_index()
        );
    }

    #[test]
    fn set_cpsr_works() {
        let mut cpu = Cpu::new();
        cpu.reset();

        cpu.set_cpsr(OperationMode::Abort.psr_mode_bits() | PSR_OP_THUMB_MASK);
        assert_eq!(RunState::Running, cpu.run_state);
        assert_eq!(OperationMode::Abort, cpu.op_mode);
        assert_eq!(OperationState::Thumb, cpu.op_state);

        cpu.set_cpsr(OperationMode::Undefined.psr_mode_bits());
        assert_eq!(RunState::Running, cpu.run_state);
        assert_eq!(OperationMode::Undefined, cpu.op_mode);
        assert_eq!(OperationState::Arm, cpu.op_state);

        // invalid mode should hang (actually, behaviour is indeterminate, but this is easier)
        cpu.set_cpsr(0);
        assert_eq!(RunState::Hung, cpu.run_state);
    }

    #[test]
    fn reset_sets_correct_values() {
        let mut cpu = Cpu::new();
        cpu.reg.r[R_PC_INDEX] = 0xdead;
        cpu.reg.cpsr = (0b1111 << 28) | 0b1111_1111;
        cpu.reset();

        assert_eq!(RunState::Running, cpu.run_state);
        assert_eq!(OperationMode::Supervisor, cpu.op_mode);
        assert_eq!(OperationState::Arm, cpu.op_state);

        // TODO: is all of PC meant to be 0, or just the least-significant 16 bits?
        assert_eq!(0, cpu.reg.r[R_PC_INDEX]);
        assert_eq!(
            OperationMode::Supervisor.psr_mode_bits(),
            cpu.reg.cpsr & PSR_OP_MODE_MASK
        );
        assert_eq!(0, cpu.reg.cpsr & PSR_OP_THUMB_MASK);
        assert_ne!(0, cpu.reg.cpsr & PSR_IRQ_DISABLE_MASK);
        assert_ne!(0, cpu.reg.cpsr & PSR_FIQ_DISABLE_MASK);
        // condition flags should be preserved
        assert_eq!(
            0b1111 << 28,
            cpu.reg.cpsr & (PSR_NEG_LT_MASK | PSR_ZERO_MASK | PSR_CARRY_MASK | PSR_OVERFLOW_MASK)
        );
    }

    #[test]
    fn change_mode_works() {
        let mut cpu = Cpu::new();
        cpu.reset();
        cpu.change_mode(OperationMode::User);

        assert_eq!(OperationMode::User, cpu.op_mode);

        cpu.reg.r = [1337; 16];
        cpu.reg.cpsr = 999;
        cpu.reg.spsr = 333;
        cpu.change_mode(OperationMode::Undefined);

        assert_eq!(OperationMode::Undefined, cpu.op_mode);
        assert!(matches!(
            cpu.reg.banks[OperationMode::User.bank_index()],
            RegisterBank {
                r13: 1337,
                r14: 1337,
                spsr: _, // doesn't matter for usr/sys
            }
        ));

        cpu.reg.r[13..=14].fill(1234);
        cpu.reg.spsr = 0xbeef;
        cpu.change_mode(OperationMode::FastInterrupt);

        assert_eq!(OperationMode::FastInterrupt, cpu.op_mode);
        assert_eq!(
            RegisterBank {
                r13: 1234,
                r14: 1234,
                spsr: 0xbeef,
            },
            cpu.reg.banks[OperationMode::Undefined.bank_index()]
        );
        // should have temporarily saved r8-r12 for restoring them later
        assert_eq!([1337; 5], cpu.reg.fiq_r8_12_bank);

        // changing to fiq mode should bank r8-r12 too
        cpu.reg.r[8..=12].fill(0xeeee);
        cpu.reg.r[13..=14].fill(0xaaaa);
        cpu.reg.spsr = 0xfe;
        cpu.change_mode(OperationMode::User);

        // been in usr mode already, so should also have the register values from when we started
        assert_eq!(OperationMode::User, cpu.op_mode);
        assert_eq!([1337; 2], cpu.reg.r[13..=14]);
        assert_eq!([0xeeee; 5], cpu.reg.fiq_r8_12_bank);
        assert_eq!(
            RegisterBank {
                r13: 0xaaaa,
                r14: 0xaaaa,
                spsr: 0xfe,
            },
            cpu.reg.banks[OperationMode::FastInterrupt.bank_index()]
        );

        // no need to do banking when switching to the same mode, or when switching between usr
        // and sys modes (they share the same "bank", which is actually no bank; that's an
        // implementation detail)
        cpu.change_mode(OperationMode::System);

        assert_eq!(OperationMode::System, cpu.op_mode);
        assert_eq!([1337; 2], cpu.reg.r[13..=14]);
        assert!(matches!(
            cpu.reg.banks[OperationMode::System.bank_index()],
            RegisterBank {
                r13: 1337,
                r14: 1337,
                spsr: _, // doesn't matter for usr/sys
            }
        ));
    }
}
