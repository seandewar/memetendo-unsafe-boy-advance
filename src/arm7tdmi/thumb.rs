use super::{reg::NamedGeneralRegister::*, reg::Registers, Cpu, Exception};

#[derive(Copy, Clone, Debug)]
enum InstructionFormat {
    MoveShiftedReg = 1,
    AddSub,
    MoveCmpAddSubImm,
    AluOp,
    HiRegOpBranchExchange,
    LoadPcRel,
    LoadStoreRel,
    LoadStoreSignExtend,
    LoadStoreImm,
    LoadStoreHword,
    LoadStoreSpRel,
    LoadAddr,
    AddSp,
    PushPopReg,
    MultiLoadStore,
    CondBranch,
    SoftwareInterrupt,
    UncondBranch,
    LongBranchWithLink,
    Undefined = 0,
}

#[must_use]
fn decode_format(instr: u16) -> InstructionFormat {
    use InstructionFormat::*;

    let hi8 = (instr >> 8) as u8;
    let hi6 = hi8 >> 2;
    let hi5 = hi8 >> 3;
    let hi4 = hi8 >> 4;
    let hi3 = hi8 >> 5;
    let bit9 = hi8 & 0b10 != 0;

    match (hi3, hi4, hi5, hi6, hi8, bit9) {
        (_, _, _, _, 0b1011_0000, _) => AddSp,
        (_, _, _, _, 0b1011_1111, _) => SoftwareInterrupt,
        (_, _, _, 0b01_0000, _, _) => AluOp,
        (_, _, _, 0b01_0001, _, _) => HiRegOpBranchExchange,
        (_, _, 0b0_0011, _, _, _) => AddSub,
        (_, _, 0b0_1001, _, _, _) => LoadPcRel,
        (_, _, 0b1_1100, _, _, _) => UncondBranch,
        (_, 0b0101, _, _, _, true) => LoadStoreSignExtend,
        (_, 0b0101, _, _, _, false) => LoadStoreRel,
        (_, 0b1000, _, _, _, _) => LoadStoreHword,
        (_, 0b1001, _, _, _, _) => LoadStoreSpRel,
        (_, 0b1010, _, _, _, _) => LoadAddr,
        (_, 0b1011, _, _, _, _) => PushPopReg,
        (_, 0b1100, _, _, _, _) => MultiLoadStore,
        (_, 0b1101, _, _, _, _) => CondBranch,
        (_, 0b1111, _, _, _, _) => LongBranchWithLink,
        (0b000, _, _, _, _, _) => MoveShiftedReg,
        (0b001, _, _, _, _, _) => MoveCmpAddSubImm,
        (0b011, _, _, _, _, _) => LoadStoreImm,
        _ => Undefined,
    }
}

impl Registers {
    #[must_use]
    pub(crate) fn pc_thumb_addr(&self) -> u32 {
        self.r[Pc] & !1
    }
}

impl Cpu {
    pub(crate) fn execute_thumb(&mut self, instr: u16) {
        use InstructionFormat::*;

        // TODO: add to CPU cycle counts when implemented
        match decode_format(instr) {
            MoveShiftedReg => {
                let r_dst = instr as usize & 0b111;
                let offset = (instr >> 6) & 0b1_1111;

                if offset > 0 {
                    let val = self.reg.r[(instr as usize >> 3) & 0b111];
                    let op = (instr >> 11) & 0b11;

                    match op {
                        // LSL, ASL
                        0b00 => {
                            self.reg.cpsr.carry = (val << (offset - 1)) & (1 << 31) != 0;
                            self.reg.r[r_dst] = val << offset;
                        }
                        // LSR, ASR
                        0b01 | 0b10 => {
                            self.reg.cpsr.carry = (val >> (offset - 1)) & 1 != 0;
                            if op == 0b01 {
                                self.reg.r[r_dst] = val >> offset;
                            } else {
                                self.reg.r[r_dst] = ((val as i32) >> offset) as _;
                            }
                        }
                        _ => unreachable!("format should be AddSub"),
                    }
                }

                self.reg.cpsr.set_zn_from(self.reg.r[r_dst]);
            }
            AddSub => todo!(),
            MoveCmpAddSubImm => todo!(),
            AluOp => todo!(),
            HiRegOpBranchExchange => todo!(),
            LoadPcRel => todo!(),
            LoadStoreRel => todo!(),
            LoadStoreSignExtend => todo!(),
            LoadStoreImm => todo!(),
            LoadStoreHword => todo!(),
            LoadStoreSpRel => todo!(),
            LoadAddr => todo!(),
            AddSp => todo!(),
            PushPopReg => todo!(),
            MultiLoadStore => todo!(),
            CondBranch => todo!(),
            SoftwareInterrupt => self.enter_exception(Exception::SoftwareInterrupt),
            UncondBranch => todo!(),
            LongBranchWithLink => todo!(),
            Undefined => self.enter_exception(Exception::UndefinedInstr),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! test_instr {
        ($before:expr, $instr:expr, $expected_rs:expr, $($expected_cspr_flags:ident)|*) => {
            let mut cpu = Cpu::new();
            cpu.reset();
            cpu.reg.cpsr.irq_disabled = false;
            cpu.reg.cpsr.fiq_disabled = false;
            $before(&mut cpu);
            cpu.execute_thumb($instr);
            assert_eq!(*cpu.reg.r, $expected_rs);

            #[allow(unused_mut)]
            let mut expected_cspr = $crate::arm7tdmi::reg::StatusRegister::default();
            $(
                test_instr!(@expand &mut expected_cspr, $expected_cspr_flags);
            )*
            assert_eq!(cpu.reg.cpsr, expected_cspr);
        };

        ($instr:expr, $expected_rs:expr, $($expected_cspr_flags:ident)|*) => {
            test_instr!(|_| {}, $instr, $expected_rs, $($expected_cspr_flags)|*);
        };

        (@expand $expected_cspr:expr, $flag:ident) => (
            $expected_cspr.$flag = true;
        );
    }

    #[test]
    fn execute_thumb_move_shifted_reg() {
        // LSL R4,R1,#3
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[1] = 0b10,
            0b000_00_00011_001_100,
            [0, 0b10, 0, 0, 0b10_000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        // LSL R0,R7,#15
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[7] = 1,
            0b000_00_01111_111_000,
            [1 << 15, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        // LSL R0,R7,#1
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[7] = 1 << 31,
            0b000_00_00001_111_000,
            [0, 0, 0, 0, 0, 0, 0, 1 << 31, 0, 0, 0, 0, 0, 0, 0, 0],
            carry | zero
        );
        // LSL R0,R7,#10
        test_instr!(
            0b000_00_01010_111_000,
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero
        );
        // LSL R0,R0,#0
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[0] = u32::MAX,
            0b000_00_00000_000_000,
            [u32::MAX, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            negative
        );

        // LSR R4,R1,#2
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[1] = 0b100,
            0b000_01_00011_001_100,
            [0, 0b100, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero | carry
        );
        // LSR R4,R1,#2
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[1] = 0b10,
            0b000_01_00011_001_100,
            [0, 0b10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero
        );
        // LSR R7,R7,#31
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[7] = 1 << 31,
            0b000_01_11111_111_111,
            [0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        // LSR R7,R7,#0
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[7] = 1 << 31,
            0b000_01_00000_111_111,
            [0, 0, 0, 0, 0, 0, 0, 1 << 31, 0, 0, 0, 0, 0, 0, 0, 0],
            negative
        );

        // ASR R7,R7,#31
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[7] = 1 << 31,
            0b000_10_11111_111_111,
            [0, 0, 0, 0, 0, 0, 0, u32::MAX, 0, 0, 0, 0, 0, 0, 0, 0],
            negative
        );
        // ASR R0,R5,#1
        #[rustfmt::skip]
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[5] = !(1 << 31),
            0b000_10_00001_101_000,
            [!(0b11 << 30), 0, 0, 0, 0, !(1 << 31), 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            carry
        );
    }
}
