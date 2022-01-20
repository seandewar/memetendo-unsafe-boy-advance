#[derive(Default, Debug)]
pub struct Cpu {
    reg: Registers,
    mode: Mode,
}

#[derive(Debug)]
#[repr(usize)]
enum DirectRegister {
    Sp = 8,
    Lr,
    Pc,
}

#[derive(Default, Debug)]
struct Registers {
    r: [u32; 16],
    cpsr: u32,
    spsr: u32,
    banks: [RegisterBank; 6],
    fiq_r8_12_bank: [u32; 5],
}

#[derive(Default, Copy, Clone, PartialEq, Eq, Debug)]
struct RegisterBank {
    r13: u32,
    r14: u32,
    spsr: u32,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Mode {
    User,
    FastInterrupt,
    Interrupt,
    Supervisor,
    Abort,
    System,
    Undefined,
}

impl Default for Mode {
    fn default() -> Self {
        Self::User
    }
}

impl Mode {
    fn bank_index(self) -> usize {
        match self {
            Mode::User | Mode::System => 0,
            Mode::FastInterrupt => 1,
            Mode::Interrupt => 2,
            Mode::Supervisor => 3,
            Mode::Abort => 4,
            Mode::Undefined => 5,
        }
    }
}

impl Cpu {
    fn change_mode(&mut self, mode: Mode) {
        let old_mode = self.mode;
        let old_bank_i = old_mode.bank_index();

        self.mode = mode;
        let bank_i = mode.bank_index();
        if old_bank_i == bank_i {
            return;
        }

        if old_mode == Mode::FastInterrupt || mode == Mode::FastInterrupt {
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
    fn bank_index() {
        assert_eq!(Mode::User.bank_index(), Mode::System.bank_index());
    }

    #[test]
    fn change_mode() {
        let mut cpu = Cpu {
            reg: Registers {
                r: [1337; 16],
                cpsr: 999,
                spsr: 333,
                ..Default::default()
            },
            mode: Mode::User,
        };
        cpu.change_mode(Mode::Undefined);

        assert_eq!(Mode::Undefined, cpu.mode);
        assert!(matches!(
            cpu.reg.banks[Mode::User.bank_index()],
            RegisterBank {
                r13: 1337,
                r14: 1337,
                spsr: _, // doesn't matter for usr/sys
            }
        ));

        cpu.reg.r[13..=14].fill(1234);
        cpu.reg.spsr = 0xbeef;
        cpu.change_mode(Mode::FastInterrupt);

        assert_eq!(Mode::FastInterrupt, cpu.mode);
        assert_eq!(
            RegisterBank {
                r13: 1234,
                r14: 1234,
                spsr: 0xbeef,
            },
            cpu.reg.banks[Mode::Undefined.bank_index()]
        );
        // should have temporarily saved r8-r12 for restoring them later
        assert_eq!([1337; 5], cpu.reg.fiq_r8_12_bank);

        // changing to fiq mode should bank r8-r12 too
        cpu.reg.r[8..=12].fill(0xeeee);
        cpu.reg.r[13..=14].fill(0xaaaa);
        cpu.reg.spsr = 0xfe;
        cpu.change_mode(Mode::User);

        // been in usr mode already, so should also have the register values from when we started
        assert_eq!(Mode::User, cpu.mode);
        assert_eq!([1337; 2], cpu.reg.r[13..=14]);
        assert_eq!([0xeeee; 5], cpu.reg.fiq_r8_12_bank);
        assert_eq!(
            RegisterBank {
                r13: 0xaaaa,
                r14: 0xaaaa,
                spsr: 0xfe,
            },
            cpu.reg.banks[Mode::FastInterrupt.bank_index()]
        );

        // no need to do banking when switching to the same mode, or when switching between usr
        // and sys modes (they share the same "bank", which is actually no bank; that's an
        // implementation detail)
        cpu.change_mode(Mode::System);
        assert_eq!(Mode::System, cpu.mode);
        assert_eq!([1337; 2], cpu.reg.r[13..=14]);
        assert!(matches!(
            cpu.reg.banks[Mode::System.bank_index()],
            RegisterBank {
                r13: 1337,
                r14: 1337,
                spsr: _, // doesn't matter for usr/sys
            }
        ));
    }
}
