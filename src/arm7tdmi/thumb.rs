use super::{
    reg::{NamedGeneralRegister::Pc, Registers},
    Cpu, Exception,
};

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
    #[allow(clippy::enum_glob_use)]
    use InstructionFormat::*;

    #[allow(clippy::cast_possible_truncation)]
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
    pub(super) fn pc_thumb_addr(&self) -> u32 {
        self.r[Pc] & !1
    }
}

impl Cpu {
    #[allow(clippy::too_many_lines)]
    pub(super) fn execute_thumb(&mut self, instr: u16) {
        #[allow(clippy::enum_glob_use)]
        use InstructionFormat::*;

        // TODO: add to CPU cycle counts when implemented
        match decode_format(instr) {
            // TODO: 1S cycle
            #[allow(clippy::cast_possible_truncation)]
            MoveShiftedReg => {
                // Rd,Rs,#Offset
                let offset = (instr >> 6) as u8 & 0b1_1111;
                let value = self.reg.r[(usize::from(instr) >> 3) & 0b111];
                let op = (instr >> 11) & 0b11;

                self.reg.r[(usize::from(instr) & 0b111)] = match op {
                    // LSL{S}
                    0 => self.execute_lsl(value, offset),
                    // LSR{S}
                    1 => self.execute_lsr(value, offset),
                    // ASR{S}
                    2 => self.execute_asr(value, offset),
                    _ => unreachable!("format should be AddSub"),
                };
            }

            // TODO: 1S cycle
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            AddSub => {
                let a = self.reg.r[(usize::from(instr) >> 3) & 0b111];
                let r_or_value = (instr >> 6) & 0b111;
                let op = (instr >> 9) & 0b11;
                let b = if op & 0b10 == 0 {
                    // Rd,Rs,Rn
                    self.reg.r[usize::from(r_or_value)]
                } else {
                    // Rd,Rs,#nn
                    r_or_value.into()
                };

                self.reg.r[(usize::from(instr) & 0b111)] = if op & 1 == 0 {
                    // ADD{S}
                    self.execute_add_cmn(a, b)
                } else {
                    // SUB{S}
                    self.execute_sub_cmp(a, b)
                };
            }

            // TODO: 1S cycle
            MoveCmpAddSubImm => {
                // Rd,#nn
                let value = u32::from(instr & 0b1111_1111);
                let r_dst = (usize::from(instr) >> 8) & 0b111;

                match (instr >> 11) & 0b11 {
                    // MOV{S}
                    0 => {
                        self.reg.r[r_dst] = self.execute_mov(value);
                    }
                    // CMP{S}
                    1 => {
                        self.execute_sub_cmp(self.reg.r[r_dst], value);
                    }
                    // ADD{S}
                    2 => {
                        self.reg.r[r_dst] = self.execute_add_cmn(self.reg.r[r_dst], value);
                    }
                    // SUB{S}
                    3 => {
                        self.reg.r[r_dst] = self.execute_sub_cmp(self.reg.r[r_dst], value);
                    }
                    _ => unreachable!(),
                }
            }

            // TODO: 1S: AND, EOR, ADC, SBC, TST, NEG, CMP, CMN, ORR, BIC, MVN
            //       1S+1I: LSL, LSR, ASR, ROR
            //       1S+mI: MUL (m=1..4; depending on MSBs of incoming Rd value)
            AluOp => {
                // Rd,Rs
                let r_dst = usize::from(instr) & 0b111;
                let value = self.reg.r[(usize::from(instr) >> 3) & 0b111];
                let op = (instr >> 6) & 0b1111;

                match op {
                    // AND{S}
                    0 => {
                        self.reg.r[r_dst] = self.execute_and_tst(self.reg.r[r_dst], value);
                    }
                    // EOR{S} (XOR)
                    1 => {
                        self.reg.r[r_dst] = self.execute_eor(self.reg.r[r_dst], value);
                    }
                    // LSL{S}
                    #[allow(clippy::cast_possible_truncation)]
                    2 => {
                        self.reg.r[r_dst] = self.execute_lsl(self.reg.r[r_dst], value as _);
                    }
                    // LSR{S}
                    #[allow(clippy::cast_possible_truncation)]
                    3 => {
                        self.reg.r[r_dst] = self.execute_lsr(self.reg.r[r_dst], value as _);
                    }
                    // ASR{S}
                    #[allow(clippy::cast_possible_truncation)]
                    4 => {
                        self.reg.r[r_dst] = self.execute_asr(self.reg.r[r_dst], value as _);
                    }
                    // ADC{S}
                    5 => {
                        self.reg.r[r_dst] = self.execute_adc(self.reg.r[r_dst], value);
                    }
                    // SBC{S}
                    6 => {
                        self.reg.r[r_dst] = self.execute_sbc(self.reg.r[r_dst], value);
                    }
                    // ROR{S}
                    #[allow(clippy::cast_possible_truncation)]
                    7 => {
                        self.reg.r[r_dst] = self.execute_ror(self.reg.r[r_dst], value as _);
                    }
                    // TST
                    8 => {
                        self.execute_and_tst(self.reg.r[r_dst], value);
                    }
                    // NEG{S}
                    9 => {
                        self.reg.r[r_dst] = self.execute_sub_cmp(0, value);
                    }
                    // CMP
                    10 => {
                        self.execute_sub_cmp(self.reg.r[r_dst], value);
                    }
                    // CMN
                    11 => {
                        self.execute_add_cmn(self.reg.r[r_dst], value);
                    }
                    // ORR{S}
                    12 => {
                        self.reg.r[r_dst] = self.execute_orr(self.reg.r[r_dst], value);
                    }
                    // MUL{S}
                    13 => {
                        self.reg.r[r_dst] = self.execute_mul(self.reg.r[r_dst], value);
                    }
                    // BIC{S}
                    14 => {
                        self.reg.r[r_dst] = self.execute_bic(self.reg.r[r_dst], value);
                    }
                    // MVN{S} (NOT)
                    15 => {
                        self.reg.r[r_dst] = self.execute_mvn(value);
                    }
                    _ => unreachable!(),
                }
            }

            // TODO: 1S cycle for ADD, MOV, CMP
            //       2S + 1N cycles for ADD, MOV with Rd=R15 and for BX
            HiRegOpBranchExchange => {
                let r_dst_no_msb = usize::from(instr) & 0b111;
                let r_src = (usize::from(instr) >> 3) & 0b1111;
                let r_dst_msb_or_bl = instr & (1 << 7) != 0;
                let op = todo!();
            }

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

#[allow(
    clippy::unusual_byte_groupings,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    clippy::unnecessary_cast // lint doesn't work properly with negative literals
)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::arm7tdmi::reg::{GeneralRegisters, StatusRegister};

    fn test_instr(
        before: impl Fn(&mut Cpu),
        instr: u16,
        expected_rs: &GeneralRegisters,
        expected_cspr: StatusRegister,
    ) {
        let mut cpu = Cpu::new();
        cpu.reset();
        cpu.reg.cpsr.irq_disabled = false;
        cpu.reg.cpsr.fiq_disabled = false;
        before(&mut cpu);
        cpu.execute_thumb(instr);

        assert_eq!(cpu.reg.r, *expected_rs);
        assert_eq!(cpu.reg.cpsr, expected_cspr);
    }

    macro_rules! test_instr {
        ($before:expr, $instr:expr, $expected_rs:expr, $($expected_cspr_flags:ident)|*) => {
            #[allow(unused_mut)]
            let mut expected_cspr = StatusRegister::default();
            $(
                test_instr!(@expand &mut expected_cspr, $expected_cspr_flags);
            )*

            test_instr($before, $instr, &GeneralRegisters($expected_rs), expected_cspr);
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
        // LSL{S} Rd,Rs,#Offset
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[1] = 0b10,
            0b000_00_00011_001_100, // LSL R4,R1,#3
            [0, 0b10, 0, 0, 0b10_000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[7] = 1,
            0b000_00_01111_111_000, // LSL R0,R7,#15
            [1 << 15, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[7] = 1 << 31,
            0b000_00_00001_111_000, // LSL R0,R7,#1
            [0, 0, 0, 0, 0, 0, 0, 1 << 31, 0, 0, 0, 0, 0, 0, 0, 0],
            carry | zero
        );
        test_instr!(
            0b000_00_01010_111_000, // LSL R0,R7,#10
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[0] = u32::MAX,
            0b000_00_00000_000_000, // LSL R0,R0,#0
            [u32::MAX, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            negative
        );

        // LSR{S} Rd,Rs,#Offset
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[1] = 0b100,
            0b000_01_00011_001_100, // LSR R4,R1,#2
            [0, 0b100, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero | carry
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[1] = 0b10,
            0b000_01_00011_001_100, // LSR R4,R1,#2
            [0, 0b10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[7] = 1 << 31,
            0b000_01_11111_111_111, // LSR R7,R7,#31
            [0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[7] = 1 << 31,
            0b000_01_00000_111_111, // LSR R7,R7,#32
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero | carry
        );

        // ASR{S} Rd,Rs,#Offset
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[7] = 1 << 31,
            0b000_10_11111_111_111, // ASR R7,R7,#31
            [0, 0, 0, 0, 0, 0, 0, u32::MAX, 0, 0, 0, 0, 0, 0, 0, 0],
            negative
        );
        #[rustfmt::skip]
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[5] = !(1 << 31),
            0b000_10_00001_101_000, // ASR R0,R5,#1
            [!(0b11 << 30), 0, 0, 0, 0, !(1 << 31), 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            carry
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[7] = 1 << 31,
            0b000_10_00000_111_111, // RSR R7,R7,#32
            [0, 0, 0, 0, 0, 0, 0, u32::MAX, 0, 0, 0, 0, 0, 0, 0, 0],
            negative | carry
        );
    }

    #[test]
    fn execute_thumb_add_sub() {
        // ADD{S} Rd,Rs,Rn
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[1] = 13;
                cpu.reg.r[7] = 7;
            },
            0b00011_00_111_001_100, // ADD R4,R1,R7
            [0, 13, 0, 0, 20, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[7] = 1;
            },
            0b00011_00_111_111_111, // ADD R7,R7,R7
            [0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[6] = u32::MAX;
                cpu.reg.r[7] = 1;
            },
            0b00011_00_111_110_000, // ADD R0,R6,R7
            [0, 0, 0, 0, 0, 0, u32::MAX, 1, 0, 0, 0, 0, 0, 0, 0, 0],
            carry | zero
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = -5 as _;
                cpu.reg.r[1] = -10 as _;
            },
            0b00011_00_000_001_010, // ADD R2,R1,R0
            [-5 as _, -10 as _, -15 as _, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            negative | carry
        );
        #[rustfmt::skip]
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = i32::MIN as _;
                cpu.reg.r[1] = -1 as _;
            },
            0b00011_00_000_001_010, // ADD R2,R1,R0
            [i32::MIN as _, -1 as _, i32::MIN.wrapping_sub(1) as _, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            carry | overflow
        );

        // SUB{S} Rd,Rs,Rn
        #[rustfmt::skip]
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[3] = i32::MIN as _;
                cpu.reg.r[6] = i32::MAX as _;
            },
            0b00011_01_110_011_000, // SUB R0,R3,R6
            [1, 0, 0, i32::MIN as _, 0, 0, i32::MAX as _, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            carry | overflow
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[0] = -5 as _,
            0b00011_01_000_000_010, // SUB R2,R0,R0
            [-5 as _, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            carry | zero
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 5;
                cpu.reg.r[1] = -10 as _;
            },
            0b00011_01_000_001_010, // SUB R2,R1,R0
            [5, -10 as _, -15 as _, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            negative | carry
        );
        #[rustfmt::skip]
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 1;
                cpu.reg.r[1] = i32::MIN as u32 + 1;
            },
            0b00011_01_000_001_010, // SUB R2,R1,R0
            [1, i32::MIN as u32 + 1, i32::MIN as _, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            negative | carry
        );

        // ADD{S} Rd,Rs,#nn
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[0] = 10,
            0b00011_10_101_000_000, // ADD R0,R0,#5
            [15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );

        // SUB{S} Rd,Rs,#nn
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[0] = 10,
            0b00011_11_010_000_000, // SUB R0,R0,#2
            [8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            carry
        );
    }

    #[test]
    fn execute_thumb_mov_cmp_add_sub_imm() {
        // MOV{S} Rd,#nn
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.cpsr.negative = true,
            0b001_00_101_11111111, // MOV R5,#255
            [0, 0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[1] = 1337,
            0b001_00_001_00000000, // MOV R1,#0
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero
        );

        // CMP{S} Rd,#nn
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[6] = 255,
            0b001_01_110_11111111, // CMP R6,#255
            [0, 0, 0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero | carry
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[2] = 13,
            0b001_01_010_00000000, // CMP R2,#0
            [0, 0, 13, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );

        // ADD{S} Rd,#nn
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[7] = 3,
            0b001_10_111_10101010, // ADD R7,#170
            [0, 0, 0, 0, 0, 0, 0, 173, 0, 0, 0, 0, 0, 0, 0, 0],
        );

        // SUB{S} Rd,#nn
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[3] = 10,
            0b001_11_011_00001111, // SUB R3,#15
            [0, 0, 0, -5 as _, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            negative
        );
    }

    #[test]
    fn execute_thumb_alu_op() {
        // AND{S} Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 0b0011;
                cpu.reg.r[1] = 0b1010;
            },
            0b010000_0000_001_000, // AND R0,R1
            [0b0010, 0b1010, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[1] = 0b1010;
            },
            0b010000_0000_001_000, // AND R0,R1
            [0, 0b1010, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero
        );
        #[rustfmt::skip]
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[1] = i32::MIN as _;
                cpu.reg.r[5] = 1 << 31;
            },
            0b010000_0000_101_001, // AND R1,R5
            [0, i32::MIN as _, 0, 0, 0, 1 << 31, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            negative
        );

        // EOR{S} Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 0b0011;
                cpu.reg.r[1] = 0b1110;
            },
            0b010000_0001_001_000, // EOR R0,R1
            [0b1101, 0b1110, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 0b1100;
                cpu.reg.r[1] = 0b1100;
            },
            0b010000_0001_000_001, // EOR R1,R0
            [0b1100, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[1] = u32::MAX;
                cpu.reg.r[7] = u32::MAX >> 1;
            },
            0b010000_0001_001_111, // EOR R7,R1
            [0, u32::MAX, 0, 0, 0, 0, 0, 1 << 31, 0, 0, 0, 0, 0, 0, 0, 0],
            negative
        );

        // LSL{S} Rd,Rs
        // this test should not panic due to shift overflow:
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[1] = 32;
                cpu.reg.r[7] = 1;
            },
            0b010000_0010_001_111, // LSL R7,R1
            [0, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero | carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[1] = 33;
                cpu.reg.r[7] = 1;
            },
            0b010000_0010_001_111, // LSL R7,R1
            [0, 33, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[1] = u8::MAX.into();
                cpu.reg.r[7] = 1;
            },
            0b010000_0010_001_111, // LSL R7,R1
            [0, u8::MAX.into(), 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero
        );

        // LSR{S} Rd,Rs
        // this test should not panic due to shift overflow:
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 32;
                cpu.reg.r[1] = 1 << 31;
            },
            0b010000_0011_000_001, // LSR R1,R0
            [32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero | carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 33;
                cpu.reg.r[1] = 1 << 31;
            },
            0b010000_0011_000_001, // LSR R1,R0
            [33, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = u8::MAX.into();
                cpu.reg.r[1] = 1;
            },
            0b010000_0011_000_001, // LSR R1,R0
            [u8::MAX.into(), 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 3;
                cpu.reg.r[1] = 0b1000;
            },
            0b010000_0011_000_001, // LSR R1,R0
            [3, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );

        // ASR{S} Rd,Rs
        // this test should not panic due to shift overflow:
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 1 << 31;
                cpu.reg.r[1] = 32;
            },
            0b010000_0100_001_000, // ASR R0,R1
            [u32::MAX, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            negative | carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 1 << 31;
                cpu.reg.r[1] = 33;
            },
            0b010000_0100_001_000, // ASR R0,R1
            [u32::MAX, 33, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            negative | carry
        );
        #[rustfmt::skip]
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 1 << 31;
                cpu.reg.r[1] = u8::MAX.into();
            },
            0b010000_0100_001_000, // ASR R0,R1
            [u32::MAX, u8::MAX.into(), 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            negative | carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 1 << 30;
                cpu.reg.r[1] = u8::MAX.into();
            },
            0b010000_0100_001_000, // ASR R0,R1
            [0, u8::MAX.into(), 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero
        );

        // ADC{S} Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 5;
                cpu.reg.r[1] = 32;
            },
            0b010000_0101_000_001, // ADC R1,R0
            [5, 37, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 5;
                cpu.reg.r[1] = 32;
                cpu.reg.cpsr.carry = true;
            },
            0b010000_0101_000_001, // ADC R1,R0
            [5, 38, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[7] = 1;
            },
            0b010000_0101_000_111, // ADC R7,R0
            [u32::MAX, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            carry | zero
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[7] = 1;
                cpu.reg.cpsr.carry = true;
            },
            0b010000_0101_000_111, // ADC R7,R0
            [u32::MAX, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0],
            carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[7] = u32::MAX;
            },
            0b010000_0101_000_111, // ADC R7,R0
            [u32::MAX, 0, 0, 0, 0, 0, 0, -2 as _, 0, 0, 0, 0, 0, 0, 0, 0],
            carry | negative
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[7] = u32::MAX;
                cpu.reg.cpsr.carry = true;
            },
            0b010000_0101_000_111, // ADC R7,R0
            [u32::MAX, 0, 0, 0, 0, 0, 0, -1 as _, 0, 0, 0, 0, 0, 0, 0, 0],
            carry | negative
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[7] = u32::MAX;
                cpu.reg.cpsr.carry = true;
            },
            0b010000_0101_000_111, // ADC R7,R0
            [u32::MAX, 0, 0, 0, 0, 0, 0, -1 as _, 0, 0, 0, 0, 0, 0, 0, 0],
            carry | negative
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.cpsr.carry = true;
            },
            0b010000_0101_000_111, // ADC R7,R0
            [u32::MAX, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            carry | zero
        );

        // SBC{S} Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 5;
                cpu.reg.r[1] = 32;
            },
            0b010000_0110_000_001, // SBC R1,R0
            [5, 26, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 5;
                cpu.reg.r[1] = 32;
                cpu.reg.cpsr.carry = true;
            },
            0b010000_0110_000_001, // SBC R1,R0
            [5, 27, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = -1 as _;
                cpu.reg.r[7] = 1;
            },
            0b010000_0110_000_111, // SBC R7,R0
            [u32::MAX, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0],
            carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = -1 as _;
                cpu.reg.r[7] = 1;
                cpu.reg.cpsr.carry = true;
            },
            0b010000_0110_000_111, // SBC R7,R0
            [u32::MAX, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[7] = i32::MIN as _,
            0b010000_0110_000_111, // SBC R7,R0
            [0, 0, 0, 0, 0, 0, 0, i32::MAX as _, 0, 0, 0, 0, 0, 0, 0, 0],
            overflow | carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = i32::MAX as _;
                cpu.reg.r[7] = i32::MIN as _;
            },
            0b010000_0110_000_111, // SBC R7,R0
            [i32::MAX as _, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            overflow | carry | zero
        );

        // ROR{S} Rd,Rs
        #[rustfmt::skip]
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 2;
                cpu.reg.r[1] = 0b1111;
            },
            0b010000_0111_000_001, // ROR R1,R0
            [2, (0b11 << 30) | 0b11, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            carry | negative
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[1] = 0b1111,
            0b010000_0111_000_001, // ROR R1,R0
            [0, 0b1111, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[2] = 255;
                cpu.reg.r[3] = 0b1111;
            },
            0b010000_0111_010_011, // ROR R3,R2
            [0, 0, 255, 0b11110, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[2] = 255,
            0b010000_0111_010_011, // ROR R3,R2
            [0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero
        );

        // TST Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[1] = 0b1111,
            0b010000_1000_000_001, // TST R1,R0
            [0, 0b1111, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 0b10000;
                cpu.reg.r[1] = 0b01111;
            },
            0b010000_1000_000_001, // TST R1,R0
            [0b10000, 0b01111, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 1;
                cpu.reg.r[1] = 1;
            },
            0b010000_1000_000_001, // TST R1,R0
            [1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 1 << 31;
                cpu.reg.r[1] = u32::MAX;
            },
            0b010000_1000_000_001, // TST R1,R0
            [1 << 31, u32::MAX, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            negative
        );

        // NEG{S} Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[3] = 30,
            0b010000_1001_011_111, // NEG R7,R3
            [0, 0, 0, 30, 0, 0, 0, -30 as _, 0, 0, 0, 0, 0, 0, 0, 0],
            negative
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[3] = 0,
            0b010000_1001_011_111, // NEG R7,R3
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[3] = -10 as _,
            0b010000_1001_011_111, // NEG R7,R3
            [0, 0, 0, -10 as _, 0, 0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        // negating i32::MIN isn't possible, and it should also set the overflow flag
        #[rustfmt::skip]
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[3] = i32::MIN as _,
            0b010000_1001_011_111, // NEG R7,R3
            [0, 0, 0, i32::MIN as _, 0, 0, 0, i32::MIN as _, 0, 0, 0, 0, 0, 0, 0, 0],
            negative | overflow
        );

        // CMP Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[3] = 30;
                cpu.reg.r[4] = 30;
            },
            0b010000_1010_011_100, // CMP R4,R3
            [0, 0, 0, 30, 30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero | carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[3] = 30;
                cpu.reg.r[4] = 20;
            },
            0b010000_1010_011_100, // CMP R4,R3
            [0, 0, 0, 30, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            negative
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[3] = 20;
                cpu.reg.r[4] = 30;
            },
            0b010000_1010_011_100, // CMP R4,R3
            [0, 0, 0, 20, 30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            carry
        );

        // CMN Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[3] = -30 as _;
                cpu.reg.r[4] = 30;
            },
            0b010000_1011_011_100, // CMN R4,R3
            [0, 0, 0, -30 as _, 30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero | carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[3] = -30 as _;
                cpu.reg.r[4] = 20;
            },
            0b010000_1011_011_100, // CMN R4,R3
            [0, 0, 0, -30 as _, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            negative
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[3] = -20 as _;
                cpu.reg.r[4] = 30;
            },
            0b010000_1011_011_100, // CMN R4,R3
            [0, 0, 0, -20 as _, 30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            carry
        );

        // ORR{S} Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[5] = 0b1010;
                cpu.reg.r[0] = 0b0101;
            },
            0b010000_1100_101_000, // ORR R0,R5
            [0b1111, 0, 0, 0, 0, 0b1010, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        test_instr!(
            0b010000_1100_101_000, // ORR R0,R5
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[4] = u32::MAX,
            0b010000_1100_100_100, // ORR R4,R4
            [0, 0, 0, 0, u32::MAX, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            negative
        );

        // MUL{S} Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 11;
                cpu.reg.r[1] = 3;
            },
            0b010000_1101_001_000, // MUL R0,R1
            [33, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 0;
                cpu.reg.r[1] = 5;
            },
            0b010000_1101_001_000, // MUL R0,R1
            [0, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = -8 as _;
                cpu.reg.r[1] = 14;
            },
            0b010000_1101_001_000, // MUL R0,R1
            [-112 as _, 14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            negative
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = -4 as _;
                cpu.reg.r[1] = -4 as _;
            },
            0b010000_1101_001_000, // MUL R0,R1
            [16, -4 as _, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );

        // BIC{S} Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 0b11111;
                cpu.reg.r[1] = 0b10101;
            },
            0b010000_1110_001_000, // BIC R0,R1
            [0b01010, 0b10101, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[1] = u32::MAX;
            },
            0b010000_1110_001_000, // BIC R0,R1
            [0, u32::MAX, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero
        );
        #[rustfmt::skip]
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[1] = u32::MAX >> 1;
            },
            0b010000_1110_001_000, // BIC R0,R1
            [1 << 31, u32::MAX >> 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            negative
        );

        // MVN{S} Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[0] = u32::MAX,
            0b010000_1111_000_000, // MVN R0,R0
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            zero
        );
        #[rustfmt::skip]
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[3] = 0b1111_0000,
            0b010000_1111_011_000, // MVN R0,R3
            [!0b1111_0000, 0, 0, 0b1111_0000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            negative
        );
    }
}
