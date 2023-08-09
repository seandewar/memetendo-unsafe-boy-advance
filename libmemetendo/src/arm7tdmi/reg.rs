use std::fmt::Display;

use intbits::Bits;
use strum_macros::FromRepr;

#[derive(Default, Copy, Clone, PartialEq, Eq, FromRepr, Debug)]
pub enum OperationMode {
    User = 0b10000,
    FastInterrupt = 0b10001,
    Interrupt = 0b10010,
    #[default]
    Supervisor = 0b10011,
    Abort = 0b10111,
    UndefinedInstr = 0b11011,
    System = 0b11111,
}

impl OperationMode {
    #[must_use]
    pub fn bits(self) -> u32 {
        self as _
    }

    // Only panics if usize is smaller than 5 bits... which is impossible.
    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn from_bits(bits: u32) -> Option<Self> {
        Self::from_repr(bits.bits(..5).try_into().unwrap())
    }

    #[must_use]
    pub fn has_spsr(self) -> bool {
        self != OperationMode::User && self != OperationMode::System
    }
}

pub const SP_INDEX: usize = 13;
pub const LR_INDEX: usize = 14;
pub const PC_INDEX: usize = 15;

#[derive(Default, Copy, Clone, Debug)]
pub struct Registers {
    pub r: [u32; 16],
    pub cpsr: StatusRegister,
    spsr: u32,
    banks: [Bank; 6],
    fiq_r8_12_bank: [u32; 5],
}

impl Display for Registers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}\ncpsr: {:08x}\nspsr: {:08x}",
            self.r
                .iter()
                .enumerate()
                .map(|(i, &x)| format!("r{i}: {x:08x}"))
                .collect::<Vec<_>>()
                .join("\n"),
            self.cpsr.bits(),
            self.spsr()
        )
    }
}

#[derive(Default, Copy, Clone, Debug)]
struct Bank {
    sp: u32,
    lr: u32,
    spsr: u32,
}

impl OperationMode {
    fn bank_index(self) -> usize {
        match self {
            Self::User | Self::System => 0,
            Self::FastInterrupt => 1,
            Self::Interrupt => 2,
            Self::Supervisor => 3,
            Self::Abort => 4,
            Self::UndefinedInstr => 5,
        }
    }
}

impl Registers {
    pub fn change_mode(&mut self, mode: OperationMode) {
        self.change_bank(mode);
        self.cpsr.mode = mode;
    }

    fn change_bank(&mut self, mode: OperationMode) {
        let old_bank_idx = self.cpsr.mode.bank_index();
        let new_bank_idx = mode.bank_index();
        if old_bank_idx == new_bank_idx {
            return;
        }

        if self.cpsr.mode == OperationMode::FastInterrupt || mode == OperationMode::FastInterrupt {
            self.fiq_r8_12_bank.swap_with_slice(&mut self.r[8..=12]);
        }
        self.banks[old_bank_idx].sp = self.r[SP_INDEX];
        self.banks[old_bank_idx].lr = self.r[LR_INDEX];
        self.banks[old_bank_idx].spsr = self.spsr;

        self.r[SP_INDEX] = self.banks[new_bank_idx].sp;
        self.r[LR_INDEX] = self.banks[new_bank_idx].lr;
        self.spsr = self.banks[new_bank_idx].spsr;
    }

    pub fn align_pc(&mut self) {
        self.r[PC_INDEX] &= match self.cpsr.state {
            OperationState::Thumb => !1,
            OperationState::Arm => !0b11,
        };
    }

    pub fn advance_pc(&mut self) {
        self.r[PC_INDEX] = self.r[PC_INDEX].wrapping_add(self.cpsr.state.instr_size());
    }

    pub fn set_cpsr(&mut self, bits: u32) {
        let new_cpsr = StatusRegister::from_bits(bits);
        self.change_bank(new_cpsr.mode());
        self.cpsr = new_cpsr;
    }

    #[must_use]
    pub fn cpsr(&self) -> &StatusRegister {
        &self.cpsr
    }

    pub fn set_spsr(&mut self, bits: u32) {
        if self.cpsr.mode().has_spsr() {
            self.spsr = bits;
        }
    }

    #[must_use]
    pub fn spsr(&self) -> u32 {
        if self.cpsr.mode().has_spsr() {
            self.spsr
        } else {
            self.cpsr.bits()
        }
    }
}

#[derive(Default, Copy, Clone, PartialEq, Eq, FromRepr, Debug)]
pub enum OperationState {
    #[default]
    Arm = 0,
    Thumb = 1 << 5,
}

impl OperationState {
    #[must_use]
    pub fn from_bits(bits: u32) -> Option<Self> {
        Self::from_repr((bits & (1 << 5)) as _)
    }

    #[must_use]
    pub fn bits(self) -> u32 {
        self as _
    }

    #[must_use]
    pub fn instr_size(self) -> u32 {
        match self {
            Self::Arm => 4,
            Self::Thumb => 2,
        }
    }
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Default, Copy, Clone, PartialEq, Eq, Debug)]
pub struct StatusRegister {
    pub signed: bool,
    pub zero: bool,
    pub carry: bool,
    pub overflow: bool,

    pub irq_disabled: bool,
    pub fiq_disabled: bool,
    pub(super) state: OperationState,
    pub(super) mode: OperationMode,
}

impl StatusRegister {
    #[must_use]
    pub fn mode(self) -> OperationMode {
        self.mode
    }

    #[must_use]
    pub fn bits(self) -> u32 {
        0.with_bit(31, self.signed)
            .with_bit(30, self.zero)
            .with_bit(29, self.carry)
            .with_bit(28, self.overflow)
            .with_bit(7, self.irq_disabled)
            .with_bit(6, self.fiq_disabled)
            | self.state.bits()
            | self.mode.bits()
    }

    pub(super) fn from_bits(bits: u32) -> Self {
        let state = OperationState::from_bits(bits).unwrap();
        // TODO: What actually happens with an invalid mode?
        let mode = OperationMode::from_bits(bits).unwrap_or(OperationMode::UndefinedInstr);

        Self {
            signed: bits.bit(31),
            zero: bits.bit(30),
            carry: bits.bit(29),
            overflow: bits.bit(28),
            irq_disabled: bits.bit(7),
            fiq_disabled: bits.bit(6),
            state,
            mode,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usr_and_sys_mode_same_bank_index() {
        assert_eq!(
            OperationMode::User.bank_index(),
            OperationMode::System.bank_index()
        );
    }

    #[test]
    fn change_mode_works() {
        let mut reg = Registers::default();
        reg.change_mode(OperationMode::User);
        assert_eq!(OperationMode::User, reg.cpsr.mode);

        reg.r = [1337; 16];
        reg.change_mode(OperationMode::UndefinedInstr);
        assert_eq!(OperationMode::UndefinedInstr, reg.cpsr.mode);

        let old_bank = reg.banks[OperationMode::User.bank_index()];
        assert_eq!(1337, old_bank.sp);
        assert_eq!(1337, old_bank.lr);

        reg.r[13..=14].fill(1234);
        reg.spsr = 0b1010_1010;
        reg.change_mode(OperationMode::FastInterrupt);
        assert_eq!(OperationMode::FastInterrupt, reg.cpsr.mode);
        let old_bank = reg.banks[OperationMode::UndefinedInstr.bank_index()];
        assert_eq!(1234, old_bank.sp);
        assert_eq!(1234, old_bank.lr);
        assert_eq!(0b1010_1010, old_bank.spsr);
        assert_eq!([1337; 5], reg.fiq_r8_12_bank); // FIQ banks R8-R12 too.

        reg.r[8..=12].fill(0xeeee);
        reg.r[13..=14].fill(0xaaaa);
        reg.change_mode(OperationMode::User);
        // We started in USR mode, so we should have the starting values for R13, R14.
        assert_eq!(OperationMode::User, reg.cpsr.mode);
        assert_eq!([1337; 2], reg.r[13..=14]);
        assert_eq!([0xeeee; 5], reg.fiq_r8_12_bank);
        let old_bank = reg.banks[OperationMode::FastInterrupt.bank_index()];
        assert_eq!(0xaaaa, old_bank.sp);
        assert_eq!(0xaaaa, old_bank.lr);

        // No need to do banking when switching to the same mode, or when switching between USR and
        // SYS modes (they share the same "bank", which is actually no bank at all).
        reg.change_mode(OperationMode::System);
        assert_eq!(OperationMode::System, reg.cpsr.mode);
        assert_eq!([1337; 2], reg.r[13..=14]);
        let bank = reg.banks[OperationMode::System.bank_index()];
        assert_eq!(1337, bank.sp);
        assert_eq!(1337, bank.lr);
    }
}
