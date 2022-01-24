use std::{
    ops::{Deref, DerefMut, Index, IndexMut},
    slice::SliceIndex,
};

use strum_macros::FromRepr;

#[derive(Copy, Clone, PartialEq, Eq, FromRepr, Debug)]
#[repr(u8)]
pub enum OperationMode {
    User = 0b10000,
    FastInterrupt = 0b10001,
    Interrupt = 0b10010,
    Supervisor = 0b10011,
    Abort = 0b10111,
    System = 0b11011,
    UndefinedInstr = 0b11111,
}

impl Default for OperationMode {
    fn default() -> Self {
        Self::Supervisor
    }
}

impl OperationMode {
    #[must_use]
    pub fn psr(&self) -> u32 {
        *self as _
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(usize)]
pub enum NamedGeneralRegister {
    Sp = 13,
    Lr = 14,
    Pc = 15,
}

#[derive(Default, Copy, Clone, PartialEq, Eq, Debug)]
pub struct GeneralRegisters(pub(crate) [u32; 16]);

impl Deref for GeneralRegisters {
    type Target = [u32; 16];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for GeneralRegisters {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<I: SliceIndex<[u32]>> Index<I> for GeneralRegisters {
    type Output = I::Output;

    fn index(&self, index: I) -> &Self::Output {
        Index::index(&self.0, index)
    }
}

impl<I: SliceIndex<[u32]>> IndexMut<I> for GeneralRegisters {
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        IndexMut::index_mut(&mut self.0, index)
    }
}

impl Index<NamedGeneralRegister> for GeneralRegisters {
    type Output = u32;

    fn index(&self, index: NamedGeneralRegister) -> &Self::Output {
        &self.0[index as usize]
    }
}

impl IndexMut<NamedGeneralRegister> for GeneralRegisters {
    fn index_mut(&mut self, index: NamedGeneralRegister) -> &mut Self::Output {
        &mut self.0[index as usize]
    }
}

#[derive(Default, Copy, Clone, Debug)]
pub(crate) struct Registers {
    pub(crate) r: GeneralRegisters,
    pub(crate) cpsr: StatusRegister,
    pub(crate) spsr: StatusRegister,
    banks: [Bank; 6],
    fiq_r8_12_bank: [u32; 5],
}

#[derive(Default, Copy, Clone, PartialEq, Eq, Debug)]
struct Bank {
    r13: u32,
    r14: u32,
    spsr: StatusRegister,
}

impl OperationMode {
    #[must_use]
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
    pub(crate) fn set_cpsr(&mut self, cpsr: u32) -> Result<(), ()> {
        self.set_mode(OperationMode::from_repr((cpsr & 0b11111) as u8).ok_or(())?);

        self.cpsr.negative = cpsr & (1 << 31) != 0;
        self.cpsr.zero = cpsr & (1 << 30) != 0;
        self.cpsr.carry = cpsr & (1 << 29) != 0;
        self.cpsr.overflow = cpsr & (1 << 28) != 0;
        self.cpsr.irq_disabled = cpsr & (1 << 7) != 0;
        self.cpsr.fiq_disabled = cpsr & (1 << 6) != 0;
        self.cpsr.thumb_enabled = cpsr & (1 << 5) != 0;

        Ok(())
    }

    pub(crate) fn set_mode(&mut self, mode: OperationMode) {
        self.change_bank(mode);
        self.cpsr.mode = mode;
    }

    fn change_bank(&mut self, mode: OperationMode) {
        let old_bank_index = self.cpsr.mode.bank_index();
        let bank_index = mode.bank_index();
        if old_bank_index == bank_index {
            return;
        }

        if self.cpsr.mode == OperationMode::FastInterrupt || mode == OperationMode::FastInterrupt {
            self.fiq_r8_12_bank.swap_with_slice(&mut self.r[8..=12]);
        }
        self.banks[old_bank_index].r13 = self.r[13];
        self.banks[old_bank_index].r14 = self.r[14];
        self.banks[old_bank_index].spsr = self.spsr;

        self.r[13] = self.banks[bank_index].r13;
        self.r[14] = self.banks[bank_index].r14;
        self.spsr = self.banks[bank_index].spsr;
    }
}

#[derive(Default, Copy, Clone, PartialEq, Eq, Debug)]
pub struct StatusRegister {
    pub(crate) negative: bool,
    pub(crate) zero: bool,
    pub(crate) carry: bool,
    pub(crate) overflow: bool,
    pub(crate) irq_disabled: bool,
    pub(crate) fiq_disabled: bool,
    pub(crate) thumb_enabled: bool,
    mode: OperationMode,
}

impl StatusRegister {
    #[must_use]
    pub fn psr(&self) -> u32 {
        let mut psr = 0;
        psr |= self.mode.psr();
        psr |= u32::from(self.negative) << 31;
        psr |= u32::from(self.zero) << 30;
        psr |= u32::from(self.carry) << 29;
        psr |= u32::from(self.overflow) << 28;
        psr |= u32::from(self.irq_disabled) << 7;
        psr |= u32::from(self.fiq_disabled) << 6;
        psr |= u32::from(self.thumb_enabled) << 5;

        psr
    }

    #[must_use]
    pub fn mode(&self) -> OperationMode {
        self.mode
    }

    pub(crate) fn set_zn_from(&mut self, result: u32) {
        self.zero = result == 0;
        self.negative = (result as i32).is_negative();
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
    fn set_mode_works() {
        let mut reg = Registers::default();
        reg.set_mode(OperationMode::User);

        assert_eq!(OperationMode::User, reg.cpsr.mode());

        *reg.r = [1337; 16];
        reg.set_mode(OperationMode::UndefinedInstr);

        assert_eq!(OperationMode::UndefinedInstr, reg.cpsr.mode());
        let old_bank = reg.banks[OperationMode::User.bank_index()];
        assert_eq!(1337, old_bank.r13);
        assert_eq!(1337, old_bank.r14);

        reg.r[13..=14].fill(1234);
        let undef_spsr_zero = reg.spsr.zero;
        reg.spsr.zero = !reg.spsr.zero;
        reg.set_mode(OperationMode::FastInterrupt);

        assert_eq!(OperationMode::FastInterrupt, reg.cpsr.mode());
        let old_bank = reg.banks[OperationMode::UndefinedInstr.bank_index()];
        assert_eq!(1234, old_bank.r13);
        assert_eq!(1234, old_bank.r14);
        assert_ne!(undef_spsr_zero, old_bank.spsr.zero);
        // should have temporarily saved r8-r12 for later restoration
        assert_eq!([1337; 5], reg.fiq_r8_12_bank);

        reg.r[8..=12].fill(0xeeee);
        reg.r[13..=14].fill(0xaaaa);
        reg.set_mode(OperationMode::User);

        // been in usr mode already, so should also have the register values from when we started
        assert_eq!(OperationMode::User, reg.cpsr.mode());
        assert_eq!([1337; 2], reg.r[13..=14]);
        assert_eq!([0xeeee; 5], reg.fiq_r8_12_bank);
        let old_bank = reg.banks[OperationMode::FastInterrupt.bank_index()];
        assert_eq!(0xaaaa, old_bank.r13);
        assert_eq!(0xaaaa, old_bank.r14);

        // no need to do banking when switching to the same mode, or when switching between usr and
        // sys modes (they share the same "bank", which is actually no bank; that's an
        // implementation detail)
        reg.set_mode(OperationMode::System);

        assert_eq!(OperationMode::System, reg.cpsr.mode());
        assert_eq!([1337; 2], reg.r[13..=14]);
        let bank = reg.banks[OperationMode::System.bank_index()];
        assert_eq!(1337, bank.r13);
        assert_eq!(1337, bank.r14);
    }
}
