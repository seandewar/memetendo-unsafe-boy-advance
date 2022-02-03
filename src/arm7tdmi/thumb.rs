use crate::{arm7tdmi::reg::OperationState, bus::DataBus};

use super::{
    reg::NamedGeneralRegister::{Pc, Sp},
    Cpu, Exception,
};

#[must_use]
fn r_index(instr: u16, pos: u8) -> usize {
    (usize::from(instr) >> usize::from(pos)) & 0b111
}

impl Cpu {
    pub(super) fn execute_thumb(&mut self, bus: &mut impl DataBus, instr: u16) {
        assert!(self.reg.cpsr.state == OperationState::Thumb);

        #[allow(clippy::cast_possible_truncation)]
        let hi8 = (instr >> 8) as u8;

        let hi6 = hi8 >> 2;
        let hi5 = hi8 >> 3;
        let hi4 = hi8 >> 4;
        let hi3 = hi8 >> 5;

        match (hi3, hi4, hi5, hi6, hi8) {
            (_, _, _, _, 0b1011_0000) => self.execute_thumb13(instr),
            (_, _, _, _, 0b1011_1111) => self.enter_exception(bus, Exception::SoftwareInterrupt),
            (_, _, _, 0b01_0000, _) => self.execute_thumb4(instr),
            (_, _, _, 0b01_0001, _) => self.execute_thumb5(bus, instr),
            (_, _, 0b0_0011, _, _) => self.execute_thumb2(instr),
            (_, _, 0b0_1001, _, _) => self.execute_thumb6(bus, instr),
            (_, _, 0b1_1100, _, _) => todo!("unconditional branch"),
            (_, 0b0101, _, _, _) => self.execute_thumb7_thumb8(bus, instr),
            (_, 0b1000, _, _, _) => self.execute_thumb10(bus, instr),
            (_, 0b1001, _, _, _) => self.execute_thumb11(bus, instr),
            (_, 0b1010, _, _, _) => self.execute_thumb12(instr),
            (_, 0b1011, _, _, _) => self.execute_thumb14(bus, instr),
            (_, 0b1100, _, _, _) => todo!("multiple load/store"),
            (_, 0b1101, _, _, _) => todo!("conditional branch"),
            (_, 0b1111, _, _, _) => todo!("long branch with link"),
            (0b000, _, _, _, _) => self.execute_thumb1(instr),
            (0b001, _, _, _, _) => self.execute_thumb3(instr),
            (0b011, _, _, _, _) => self.execute_thumb9(bus, instr),
            _ => self.enter_exception(bus, Exception::UndefinedInstr),
        }
    }

    /// Thumb.1: Move shifted register.
    fn execute_thumb1(&mut self, instr: u16) {
        // TODO: 1S cycle
        // Rd,Rs,#Offset
        let value = self.reg.r[r_index(instr, 3)];
        let offset = ((instr >> 6) & 0b1_1111) as _;
        let op = (instr >> 11) & 0b11;

        self.reg.r[r_index(instr, 0)] = match op {
            // LSL{S}
            0 => self.execute_lsl(value, offset),
            // LSR{S}
            1 => self.execute_lsr(value, offset),
            // ASR{S}
            2 => self.execute_asr(value, offset),
            _ => unreachable!(),
        };
    }

    /// Thumb.2: Add or subtract.
    fn execute_thumb2(&mut self, instr: u16) {
        // TODO: 1S cycle
        let a = self.reg.r[r_index(instr, 3)];
        let r = r_index(instr, 6);
        let op = (instr >> 9) & 0b11;

        #[allow(clippy::cast_possible_truncation)]
        let b = r as _;

        self.reg.r[r_index(instr, 0)] = match op {
            // ADD{S} Rd,Rs,Rn
            0 => self.execute_add_cmn(true, a, self.reg.r[r]),
            // SUB{S} Rd,Rs,Rn
            1 => self.execute_sub_cmp(true, a, self.reg.r[r]),
            // ADD{S} Rd,Rs,#nn
            2 => self.execute_add_cmn(true, a, b),
            // SUB{S} Rd,Rs,#nn
            3 => self.execute_sub_cmp(true, a, b),
            _ => unreachable!(),
        };
    }

    /// Thumb.3: Move, compare, add or subtract immediate.
    fn execute_thumb3(&mut self, instr: u16) {
        // TODO: 1S cycle
        // Rd,#nn
        let value = (instr & 0b1111_1111).into();
        let r_dst = r_index(instr, 8);
        let op = (instr >> 11) & 0b11;

        match op {
            // MOV{S}
            0 => self.reg.r[r_dst] = self.execute_mov(true, value),
            // CMP{S}
            1 => {
                self.execute_sub_cmp(true, self.reg.r[r_dst], value);
            }
            // ADD{S}
            2 => self.reg.r[r_dst] = self.execute_add_cmn(true, self.reg.r[r_dst], value),
            // SUB{S}
            3 => self.reg.r[r_dst] = self.execute_sub_cmp(true, self.reg.r[r_dst], value),
            _ => unreachable!(),
        }
    }

    /// Thumb.4: ALU operations.
    fn execute_thumb4(&mut self, instr: u16) {
        // TODO: 1S: AND, EOR, ADC, SBC, TST, NEG, CMP, CMN, ORR, BIC, MVN
        //       1S+1I: LSL, LSR, ASR, ROR
        //       1S+mI: MUL (m=1..4; depending on MSBs of incoming Rd value)
        // Rd,Rs
        let r_dst = r_index(instr, 0);
        let value = self.reg.r[r_index(instr, 3)];
        let op = (instr >> 6) & 0b1111;

        #[allow(clippy::cast_possible_truncation)]
        let offset = value as _;

        match op {
            // AND{S}
            0 => self.reg.r[r_dst] = self.execute_and_tst(self.reg.r[r_dst], value),
            // EOR{S} (XOR)
            1 => self.reg.r[r_dst] = self.execute_eor(self.reg.r[r_dst], value),
            // LSL{S}
            2 => self.reg.r[r_dst] = self.execute_lsl(self.reg.r[r_dst], offset),
            // LSR{S}
            3 => self.reg.r[r_dst] = self.execute_lsr(self.reg.r[r_dst], offset),
            // ASR{S}
            4 => self.reg.r[r_dst] = self.execute_asr(self.reg.r[r_dst], offset),
            // ADC{S}
            5 => self.reg.r[r_dst] = self.execute_adc(true, self.reg.r[r_dst], value),
            // SBC{S}
            6 => self.reg.r[r_dst] = self.execute_sbc(true, self.reg.r[r_dst], value),
            // ROR{S}
            7 => self.reg.r[r_dst] = self.execute_ror(self.reg.r[r_dst], offset),
            // TST
            8 => {
                self.execute_and_tst(self.reg.r[r_dst], value);
            }
            // NEG{S}
            9 => self.reg.r[r_dst] = self.execute_sub_cmp(true, 0, value),
            // CMP
            10 => {
                self.execute_sub_cmp(true, self.reg.r[r_dst], value);
            }
            // CMN
            11 => {
                self.execute_add_cmn(true, self.reg.r[r_dst], value);
            }
            // ORR{S}
            12 => self.reg.r[r_dst] = self.execute_orr(self.reg.r[r_dst], value),
            // MUL{S}
            13 => self.reg.r[r_dst] = self.execute_mul(self.reg.r[r_dst], value),
            // BIC{S}
            14 => self.reg.r[r_dst] = self.execute_bic(self.reg.r[r_dst], value),
            // MVN{S} (NOT)
            15 => self.reg.r[r_dst] = self.execute_mvn(value),
            _ => unreachable!(),
        }
    }

    /// Thumb.5: Hi register operations or branch exchange.
    fn execute_thumb5(&mut self, bus: &mut impl DataBus, instr: u16) {
        // TODO: 1S cycle for ADD, MOV, CMP
        //       2S + 1N cycles for ADD, MOV with Rd=R15 and for BX
        let r_src_msb = instr & (1 << 6) != 0;
        let r_src = r_index(instr, 3) | (usize::from(r_src_msb) << 3);

        let value = self.reg.r[r_src];
        let op = (instr >> 8) & 0b11;

        if op == 3 {
            // BX Rs (jump)
            self.execute_bx(bus, value);
            return;
        }

        // Rd,Rs
        let r_dst_msb = instr & (1 << 7) != 0;
        let r_dst = r_index(instr, 0) | (usize::from(r_dst_msb) << 3);

        match op {
            // ADD
            0 => self.reg.r[r_dst] = self.execute_add_cmn(false, self.reg.r[r_dst], value),
            // CMP
            1 => {
                self.execute_sub_cmp(true, self.reg.r[r_dst], value);
            }
            // MOV or NOP (MOV R8,R8)
            2 => self.reg.r[r_dst] = self.execute_mov(false, value),
            _ => unreachable!(),
        }

        if op != 1 && r_dst == Pc as _ {
            self.reload_pipeline(bus);
        }
    }

    /// Thumb.6: Load PC relative.
    fn execute_thumb6(&mut self, bus: &mut impl DataBus, instr: u16) {
        // TODO: 1S + 1N + 1I
        // LDR Rd,[PC,#nn]
        let r_dst = r_index(instr, 8);
        let offset = u32::from(instr & 0b1111_1111);
        let addr = self.reg.r[Pc].wrapping_add(offset * 4);

        self.reg.r[r_dst] = Self::execute_ldr(bus, addr);
    }

    /// Thumb.7: Load or store with register offset, OR
    /// Thumb.8: Load or store sign-extended byte or half-word (if bit 9 is set in `instr`).
    fn execute_thumb7_thumb8(&mut self, bus: &mut impl DataBus, instr: u16) {
        // TODO: 1S + 1N + 1I for LDR, 2N for STR
        // Rd,[Rb,Ro]
        let r = r_index(instr, 0);

        let base_addr = self.reg.r[r_index(instr, 3)];
        let offset = self.reg.r[r_index(instr, 6)];
        let addr = base_addr.wrapping_add(offset);

        let thumb7 = instr & (1 << 9) == 0;
        let op = (instr >> 10) & 0b11;

        if thumb7 {
            match op {
                // STR
                0 => Self::execute_str(bus, addr, self.reg.r[r]),
                // STRB
                1 => Self::execute_strb(bus, addr, (self.reg.r[r] & 0xff) as _),
                // LDR
                2 => self.reg.r[r] = Self::execute_ldr(bus, addr),
                // LDRB
                3 => self.reg.r[r] = Self::execute_ldrb_ldsb(bus, addr, false),
                _ => unreachable!(),
            }
        } else {
            match op {
                // STRH
                0 => Self::execute_strh(bus, addr, (self.reg.r[r] & 0xffff) as _),
                // LDSB
                1 => self.reg.r[r] = Self::execute_ldrb_ldsb(bus, addr, true),
                // LDRH, LDSH
                2 | 3 => self.reg.r[r] = Self::execute_ldrh_ldsh(bus, addr, op == 3),
                _ => unreachable!(),
            }
        }
    }

    /// Thumb.9: Load or store with immediate offset.
    fn execute_thumb9(&mut self, bus: &mut impl DataBus, instr: u16) {
        // TODO: 1S+1N+1I for LDR, or 2N for STR
        // Rd,[Rb,#nn]
        let r = r_index(instr, 0);

        let base_addr = self.reg.r[r_index(instr, 3)];
        let offset = u32::from((instr >> 6) & 0b1_1111);
        let addr = base_addr.wrapping_add(offset);
        let word_addr = base_addr.wrapping_add(offset * 4);

        let op = (instr >> 11) & 0b11;

        match op {
            // STR
            0 => Self::execute_str(bus, word_addr, self.reg.r[r]),
            // LDR
            1 => self.reg.r[r] = Self::execute_ldr(bus, word_addr),
            // STRB
            2 => Self::execute_strb(bus, addr, (self.reg.r[r] & 0xff) as _),
            // LDRB
            3 => self.reg.r[r] = Self::execute_ldrb_ldsb(bus, addr, false),
            _ => unreachable!(),
        }
    }

    /// Thumb.10: Load or store half-word.
    fn execute_thumb10(&mut self, bus: &mut impl DataBus, instr: u16) {
        // 1S+1N+1I for LDR, or 2N for STR
        // Rd,[Rb,#nn]
        let r = r_index(instr, 0);

        let base_addr = self.reg.r[r_index(instr, 3)];
        let offset = u32::from((instr >> 6) & 0b1_1111);
        let addr = base_addr.wrapping_add(offset * 2);

        let op = (instr >> 11) & 1;

        match op {
            // STRH
            0 => Self::execute_strh(bus, addr, (self.reg.r[r] & 0xffff) as _),
            // LDRH
            1 => self.reg.r[r] = Self::execute_ldrh_ldsh(bus, addr, false),
            _ => unreachable!(),
        }
    }

    /// Thumb.11: Load or store SP relative.
    fn execute_thumb11(&mut self, bus: &mut impl DataBus, instr: u16) {
        // 1S+1N+1I for LDR, or 2N for STR
        // Rd,[SP,#nn]
        let offset = u32::from(instr & 0b1111_1111);
        let addr = self.reg.r[Sp].wrapping_add(offset * 4);

        let r = r_index(instr, 8);
        let op = (instr >> 11) & 1;

        match op {
            // STR
            0 => Self::execute_str(bus, addr, self.reg.r[r]),
            // LDR
            1 => self.reg.r[r] = Self::execute_ldr(bus, addr),
            _ => unreachable!(),
        }
    }

    /// Thumb.12: Get relative address.
    fn execute_thumb12(&mut self, instr: u16) {
        // TODO: 1S
        // ADD Rd,(PC/SP),#nn
        let offset = (instr & 0b1111_1111).into();
        let r_dst = r_index(instr, 8);
        let op = (instr >> 11) & 1;
        let base_addr = self.reg.r[if op == 0 { Pc } else { Sp }];

        self.reg.r[r_dst] = self.execute_add_cmn(false, base_addr, offset);
    }

    /// Thumb.13: Add offset to SP.
    fn execute_thumb13(&mut self, instr: u16) {
        // TODO: 1S
        // SP,#nn
        let offset = ((instr & 0b111_1111) * 4).into();
        let op = (instr >> 7) & 1;

        self.reg.r[Sp] = match op {
            // ADD
            0 => self.execute_add_cmn(false, self.reg.r[Sp], offset),
            // SUB
            1 => self.execute_sub_cmp(false, self.reg.r[Sp], offset),
            _ => unreachable!(),
        };
    }

    /// Thumb.14: Push or pop registers.
    fn execute_thumb14(&mut self, bus: &mut impl DataBus, instr: u16) {
        // TODO: nS+1N+1I (POP), (n+1)S+2N+1I (POP PC), or (n-1)S+2N (PUSH)
        let r_list = (instr & 0b1111_1111) as _;
        let push_lr_pop_pc = instr & (1 << 8) != 0;
        let op = (instr >> 11) & 1;

        match op {
            // PUSH {Rlist}{LR}
            0 => self.execute_push(bus, r_list, push_lr_pop_pc),
            // POP {Rlist}{PC}
            1 => self.execute_pop(bus, r_list, push_lr_pop_pc),
            _ => unreachable!(),
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

    use crate::{
        arm7tdmi::reg::{GeneralRegisters, NamedGeneralRegister::Lr, StatusRegister},
        bus::{NullBus, VecBus},
    };

    fn test_instr(
        bus: &mut impl DataBus,
        before: impl Fn(&mut Cpu),
        instr: u16,
        expected_rs: &GeneralRegisters,
        expected_cspr: StatusRegister,
    ) {
        let mut cpu = Cpu::new();
        cpu.reset(bus);

        // act like the CPU started in THUMB mode with interrupts enabled
        cpu.reg.cpsr.irq_disabled = false;
        cpu.reg.cpsr.fiq_disabled = false;
        cpu.execute_bx(bus, 0);
        before(&mut cpu);
        cpu.execute_thumb(bus, instr);

        assert_eq!(cpu.reg.r, *expected_rs);

        // only check condition and interrupt flags
        assert_eq!(
            cpu.reg.cpsr.negative, expected_cspr.negative,
            "negative flag"
        );
        assert_eq!(cpu.reg.cpsr.zero, expected_cspr.zero, "zero flag");
        assert_eq!(cpu.reg.cpsr.carry, expected_cspr.carry, "carry flag");
        assert_eq!(
            cpu.reg.cpsr.overflow, expected_cspr.overflow,
            "overflow flag"
        );
        assert_eq!(
            cpu.reg.cpsr.irq_disabled, expected_cspr.irq_disabled,
            "irq_disabled flag"
        );
        assert_eq!(
            cpu.reg.cpsr.fiq_disabled, expected_cspr.fiq_disabled,
            "fiq_disabled flag"
        );
    }

    macro_rules! test_instr {
        (
            $bus:expr,
            $before:expr,
            $instr:expr,
            $expected_rs:expr,
            $($expected_cspr_flag:ident)|*
        ) => {
            let mut expected_cpsr = StatusRegister::default();
            expected_cpsr.state = OperationState::Thumb;
            $(
                test_instr!(@expand &mut expected_cpsr, $expected_cspr_flag);
            )*

            test_instr($bus, $before, $instr, &GeneralRegisters($expected_rs), expected_cpsr);
        };

        ($before:expr, $instr:expr, $expected_rs:expr, $($expected_cspr_flag:ident)|*) => {
            test_instr!(&mut NullBus, $before, $instr, $expected_rs, $($expected_cspr_flag)|*);
        };

        ($instr:expr, $expected_rs:expr, $($expected_cspr_flag:ident)|*) => {
            test_instr!(&mut NullBus, |_| {}, $instr, $expected_rs, $($expected_cspr_flag)|*);
        };

        (@expand $expected_cspr:expr, $flag:ident) => (
            $expected_cspr.$flag = true;
        );
    }

    #[test]
    fn execute_thumb1() {
        // LSL{S} Rd,Rs,#Offset
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[1] = 0b10,
            0b000_00_00011_001_100, // LSL R4,R1,#3
            [0, 0b10, 0, 0, 0b10_000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[7] = 1,
            0b000_00_01111_111_000, // LSL R0,R7,#15
            [1 << 15, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[7] = 1 << 31,
            0b000_00_00001_111_000, // LSL R0,R7,#1
            [0, 0, 0, 0, 0, 0, 0, 1 << 31, 0, 0, 0, 0, 0, 0, 0, 4],
            carry | zero
        );
        test_instr!(
            0b000_00_01010_111_000, // LSL R0,R7,#10
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[0] = u32::MAX,
            0b000_00_00000_000_000, // LSL R0,R0,#0
            [u32::MAX, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            negative
        );

        // LSR{S} Rd,Rs,#Offset
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[1] = 0b100,
            0b000_01_00011_001_100, // LSR R4,R1,#2
            [0, 0b100, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero | carry
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[1] = 0b10,
            0b000_01_00011_001_100, // LSR R4,R1,#2
            [0, 0b10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[7] = 1 << 31,
            0b000_01_11111_111_111, // LSR R7,R7,#31
            [0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[7] = 1 << 31,
            0b000_01_00000_111_111, // LSR R7,R7,#32
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero | carry
        );

        // ASR{S} Rd,Rs,#Offset
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[7] = 1 << 31,
            0b000_10_11111_111_111, // ASR R7,R7,#31
            [0, 0, 0, 0, 0, 0, 0, u32::MAX, 0, 0, 0, 0, 0, 0, 0, 4],
            negative
        );
        #[rustfmt::skip]
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[5] = !(1 << 31),
            0b000_10_00001_101_000, // ASR R0,R5,#1
            [!(0b11 << 30), 0, 0, 0, 0, !(1 << 31), 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            carry
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[7] = 1 << 31,
            0b000_10_00000_111_111, // RSR R7,R7,#32
            [0, 0, 0, 0, 0, 0, 0, u32::MAX, 0, 0, 0, 0, 0, 0, 0, 4],
            negative | carry
        );
    }

    #[test]
    fn execute_thumb2() {
        // ADD{S} Rd,Rs,Rn
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[1] = 13;
                cpu.reg.r[7] = 7;
            },
            0b00011_00_111_001_100, // ADD R4,R1,R7
            [0, 13, 0, 0, 20, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[7] = 1;
            },
            0b00011_00_111_111_111, // ADD R7,R7,R7
            [0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[6] = u32::MAX;
                cpu.reg.r[7] = 1;
            },
            0b00011_00_111_110_000, // ADD R0,R6,R7
            [0, 0, 0, 0, 0, 0, u32::MAX, 1, 0, 0, 0, 0, 0, 0, 0, 4],
            carry | zero
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = -5 as _;
                cpu.reg.r[1] = -10 as _;
            },
            0b00011_00_000_001_010, // ADD R2,R1,R0
            [-5 as _, -10 as _, -15 as _, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            negative | carry
        );
        #[rustfmt::skip]
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = i32::MIN as _;
                cpu.reg.r[1] = -1 as _;
            },
            0b00011_00_000_001_010, // ADD R2,R1,R0
            [i32::MIN as _, -1 as _, i32::MIN.wrapping_sub(1) as _, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
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
            [1, 0, 0, i32::MIN as _, 0, 0, i32::MAX as _, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            carry | overflow
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[0] = -5 as _,
            0b00011_01_000_000_010, // SUB R2,R0,R0
            [-5 as _, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            carry | zero
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 5;
                cpu.reg.r[1] = -10 as _;
            },
            0b00011_01_000_001_010, // SUB R2,R1,R0
            [5, -10 as _, -15 as _, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            negative | carry
        );
        #[rustfmt::skip]
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 1;
                cpu.reg.r[1] = i32::MIN as u32 + 1;
            },
            0b00011_01_000_001_010, // SUB R2,R1,R0
            [1, i32::MIN as u32 + 1, i32::MIN as _, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            negative | carry
        );

        // ADD{S} Rd,Rs,#nn
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[0] = 10,
            0b00011_10_101_000_000, // ADD R0,R0,#5
            [15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );

        // SUB{S} Rd,Rs,#nn
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[0] = 10,
            0b00011_11_010_000_000, // SUB R0,R0,#2
            [8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            carry
        );
    }

    #[test]
    fn execute_thumb3() {
        // MOV{S} Rd,#nn
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.cpsr.negative = true,
            0b001_00_101_11111111, // MOV R5,#255
            [0, 0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[1] = 1337,
            0b001_00_001_00000000, // MOV R1,#0
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero
        );

        // CMP{S} Rd,#nn
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[6] = 255,
            0b001_01_110_11111111, // CMP R6,#255
            [0, 0, 0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero | carry
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[2] = 13,
            0b001_01_010_00000000, // CMP R2,#0
            [0, 0, 13, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );

        // ADD{S} Rd,#nn
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[7] = 3,
            0b001_10_111_10101010, // ADD R7,#170
            [0, 0, 0, 0, 0, 0, 0, 173, 0, 0, 0, 0, 0, 0, 0, 4],
        );

        // SUB{S} Rd,#nn
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[3] = 10,
            0b001_11_011_00001111, // SUB R3,#15
            [0, 0, 0, -5 as _, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            negative
        );
    }

    #[test]
    fn execute_thumb4() {
        // AND{S} Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 0b0011;
                cpu.reg.r[1] = 0b1010;
            },
            0b010000_0000_001_000, // AND R0,R1
            [0b0010, 0b1010, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[1] = 0b1010,
            0b010000_0000_001_000, // AND R0,R1
            [0, 0b1010, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero
        );
        #[rustfmt::skip]
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[1] = i32::MIN as _;
                cpu.reg.r[5] = 1 << 31;
            },
            0b010000_0000_101_001, // AND R1,R5
            [0, i32::MIN as _, 0, 0, 0, 1 << 31, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            negative
        );

        // EOR{S} Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 0b0011;
                cpu.reg.r[1] = 0b1110;
            },
            0b010000_0001_001_000, // EOR R0,R1
            [0b1101, 0b1110, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 0b1100;
                cpu.reg.r[1] = 0b1100;
            },
            0b010000_0001_000_001, // EOR R1,R0
            [0b1100, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[1] = u32::MAX;
                cpu.reg.r[7] = u32::MAX >> 1;
            },
            0b010000_0001_001_111, // EOR R7,R1
            [0, u32::MAX, 0, 0, 0, 0, 0, 1 << 31, 0, 0, 0, 0, 0, 0, 0, 4],
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
            [0, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero | carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[1] = 33;
                cpu.reg.r[7] = 1;
            },
            0b010000_0010_001_111, // LSL R7,R1
            [0, 33, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[1] = u8::MAX.into();
                cpu.reg.r[7] = 1;
            },
            0b010000_0010_001_111, // LSL R7,R1
            [0, u8::MAX.into(), 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
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
            [32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero | carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 33;
                cpu.reg.r[1] = 1 << 31;
            },
            0b010000_0011_000_001, // LSR R1,R0
            [33, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = u8::MAX.into();
                cpu.reg.r[1] = 1;
            },
            0b010000_0011_000_001, // LSR R1,R0
            [u8::MAX.into(), 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 3;
                cpu.reg.r[1] = 0b1000;
            },
            0b010000_0011_000_001, // LSR R1,R0
            [3, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );

        // ASR{S} Rd,Rs
        // this test should not panic due to shift overflow:
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 1 << 31;
                cpu.reg.r[1] = 32;
            },
            0b010000_0100_001_000, // ASR R0,R1
            [u32::MAX, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            negative | carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 1 << 31;
                cpu.reg.r[1] = 33;
            },
            0b010000_0100_001_000, // ASR R0,R1
            [u32::MAX, 33, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            negative | carry
        );
        #[rustfmt::skip]
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 1 << 31;
                cpu.reg.r[1] = u8::MAX.into();
            },
            0b010000_0100_001_000, // ASR R0,R1
            [u32::MAX, u8::MAX.into(), 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            negative | carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 1 << 30;
                cpu.reg.r[1] = u8::MAX.into();
            },
            0b010000_0100_001_000, // ASR R0,R1
            [0, u8::MAX.into(), 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero
        );

        // ADC{S} Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 5;
                cpu.reg.r[1] = 32;
            },
            0b010000_0101_000_001, // ADC R1,R0
            [5, 37, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 5;
                cpu.reg.r[1] = 32;
                cpu.reg.cpsr.carry = true;
            },
            0b010000_0101_000_001, // ADC R1,R0
            [5, 38, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[7] = 1;
            },
            0b010000_0101_000_111, // ADC R7,R0
            [u32::MAX, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            carry | zero
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[7] = 1;
                cpu.reg.cpsr.carry = true;
            },
            0b010000_0101_000_111, // ADC R7,R0
            [u32::MAX, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 4],
            carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[7] = u32::MAX;
            },
            0b010000_0101_000_111, // ADC R7,R0
            [u32::MAX, 0, 0, 0, 0, 0, 0, -2 as _, 0, 0, 0, 0, 0, 0, 0, 4],
            carry | negative
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[7] = u32::MAX;
                cpu.reg.cpsr.carry = true;
            },
            0b010000_0101_000_111, // ADC R7,R0
            [u32::MAX, 0, 0, 0, 0, 0, 0, -1 as _, 0, 0, 0, 0, 0, 0, 0, 4],
            carry | negative
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[7] = u32::MAX;
                cpu.reg.cpsr.carry = true;
            },
            0b010000_0101_000_111, // ADC R7,R0
            [u32::MAX, 0, 0, 0, 0, 0, 0, -1 as _, 0, 0, 0, 0, 0, 0, 0, 4],
            carry | negative
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.cpsr.carry = true;
            },
            0b010000_0101_000_111, // ADC R7,R0
            [u32::MAX, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            carry | zero
        );

        // SBC{S} Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 5;
                cpu.reg.r[1] = 32;
            },
            0b010000_0110_000_001, // SBC R1,R0
            [5, 26, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 5;
                cpu.reg.r[1] = 32;
                cpu.reg.cpsr.carry = true;
            },
            0b010000_0110_000_001, // SBC R1,R0
            [5, 27, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = -1 as _;
                cpu.reg.r[7] = 1;
            },
            0b010000_0110_000_111, // SBC R7,R0
            [u32::MAX, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 4],
            carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = -1 as _;
                cpu.reg.r[7] = 1;
                cpu.reg.cpsr.carry = true;
            },
            0b010000_0110_000_111, // SBC R7,R0
            [u32::MAX, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[7] = i32::MIN as _,
            0b010000_0110_000_111, // SBC R7,R0
            [0, 0, 0, 0, 0, 0, 0, i32::MAX as _, 0, 0, 0, 0, 0, 0, 0, 4],
            overflow | carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = i32::MAX as _;
                cpu.reg.r[7] = i32::MIN as _;
            },
            0b010000_0110_000_111, // SBC R7,R0
            [i32::MAX as _, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
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
            [2, (0b11 << 30) | 0b11, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            carry | negative
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[1] = 0b1111,
            0b010000_0111_000_001, // ROR R1,R0
            [0, 0b1111, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[2] = 255;
                cpu.reg.r[3] = 0b1111;
            },
            0b010000_0111_010_011, // ROR R3,R2
            [0, 0, 255, 0b11110, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[2] = 255,
            0b010000_0111_010_011, // ROR R3,R2
            [0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero
        );

        // TST Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[1] = 0b1111,
            0b010000_1000_000_001, // TST R1,R0
            [0, 0b1111, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 0b10000;
                cpu.reg.r[1] = 0b01111;
            },
            0b010000_1000_000_001, // TST R1,R0
            [0b10000, 0b01111, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 1;
                cpu.reg.r[1] = 1;
            },
            0b010000_1000_000_001, // TST R1,R0
            [1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 1 << 31;
                cpu.reg.r[1] = u32::MAX;
            },
            0b010000_1000_000_001, // TST R1,R0
            [1 << 31, u32::MAX, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            negative
        );

        // NEG{S} Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[3] = 30,
            0b010000_1001_011_111, // NEG R7,R3
            [0, 0, 0, 30, 0, 0, 0, -30 as _, 0, 0, 0, 0, 0, 0, 0, 4],
            negative
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[3] = 0,
            0b010000_1001_011_111, // NEG R7,R3
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[3] = -10 as _,
            0b010000_1001_011_111, // NEG R7,R3
            [0, 0, 0, -10 as _, 0, 0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        // negating i32::MIN isn't possible, and it should also set the overflow flag
        #[rustfmt::skip]
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[3] = i32::MIN as _,
            0b010000_1001_011_111, // NEG R7,R3
            [0, 0, 0, i32::MIN as _, 0, 0, 0, i32::MIN as _, 0, 0, 0, 0, 0, 0, 0, 4],
            negative | overflow
        );

        // CMP Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[3] = 30;
                cpu.reg.r[4] = 30;
            },
            0b010000_1010_011_100, // CMP R4,R3
            [0, 0, 0, 30, 30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero | carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[3] = 30;
                cpu.reg.r[4] = 20;
            },
            0b010000_1010_011_100, // CMP R4,R3
            [0, 0, 0, 30, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            negative
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[3] = 20;
                cpu.reg.r[4] = 30;
            },
            0b010000_1010_011_100, // CMP R4,R3
            [0, 0, 0, 20, 30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            carry
        );

        // CMN Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[3] = -30 as _;
                cpu.reg.r[4] = 30;
            },
            0b010000_1011_011_100, // CMN R4,R3
            [0, 0, 0, -30 as _, 30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero | carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[3] = -30 as _;
                cpu.reg.r[4] = 20;
            },
            0b010000_1011_011_100, // CMN R4,R3
            [0, 0, 0, -30 as _, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            negative
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[3] = -20 as _;
                cpu.reg.r[4] = 30;
            },
            0b010000_1011_011_100, // CMN R4,R3
            [0, 0, 0, -20 as _, 30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            carry
        );

        // ORR{S} Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[5] = 0b1010;
                cpu.reg.r[0] = 0b0101;
            },
            0b010000_1100_101_000, // ORR R0,R5
            [0b1111, 0, 0, 0, 0, 0b1010, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        test_instr!(
            0b010000_1100_101_000, // ORR R0,R5
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[4] = u32::MAX,
            0b010000_1100_100_100, // ORR R4,R4
            [0, 0, 0, 0, u32::MAX, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            negative
        );

        // MUL{S} Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 11;
                cpu.reg.r[1] = 3;
            },
            0b010000_1101_001_000, // MUL R0,R1
            [33, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 0;
                cpu.reg.r[1] = 5;
            },
            0b010000_1101_001_000, // MUL R0,R1
            [0, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = -8 as _;
                cpu.reg.r[1] = 14;
            },
            0b010000_1101_001_000, // MUL R0,R1
            [-112 as _, 14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            negative
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = -4 as _;
                cpu.reg.r[1] = -4 as _;
            },
            0b010000_1101_001_000, // MUL R0,R1
            [16, -4 as _, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );

        // BIC{S} Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 0b11111;
                cpu.reg.r[1] = 0b10101;
            },
            0b010000_1110_001_000, // BIC R0,R1
            [0b01010, 0b10101, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[1] = u32::MAX;
            },
            0b010000_1110_001_000, // BIC R0,R1
            [0, u32::MAX, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero
        );
        #[rustfmt::skip]
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[1] = u32::MAX >> 1;
            },
            0b010000_1110_001_000, // BIC R0,R1
            [1 << 31, u32::MAX >> 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            negative
        );

        // MVN{S} Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[0] = u32::MAX,
            0b010000_1111_000_000, // MVN R0,R0
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            zero
        );
        #[rustfmt::skip]
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[3] = 0b1111_0000,
            0b010000_1111_011_000, // MVN R0,R3
            [!0b1111_0000, 0, 0, 0b1111_0000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
            negative
        );
    }

    #[test]
    fn execute_thumb5() {
        // ADD Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[13] = 20;
                cpu.reg.r[1] = 15;
            },
            0b010001_00_1_0_001_101, // ADD R13,R1
            [0, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 35, 0, 4],
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[8] = 5;
                cpu.reg.r[14] = -10 as _;
            },
            0b010001_00_1_1_110_000, // ADD R8,R14
            [0, 0, 0, 0, 0, 0, 0, 0, -5 as _, 0, 0, 0, 0, 0, -10 as _, 4],
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[Pc] = 1;
                cpu.reg.r[10] = 10;
            },
            0b010001_00_1_1_010_111, // ADD PC,R10
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 0, 0, 0, 0, 14],
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[Pc] = 0;
                cpu.reg.r[10] = 10;
            },
            0b010001_00_1_1_010_111, // ADD PC,R10
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 0, 0, 0, 0, 14],
        );

        // CMP Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[13] = 20;
                cpu.reg.r[1] = 15;
            },
            0b010001_01_1_0_001_101, // CMP R13,R1
            [0, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 20, 0, 4],
            carry
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[13] = 20;
                cpu.reg.r[1] = 15;
            },
            0b010001_01_0_1_101_001, // CMP R1,R13
            [0, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 20, 0, 4],
            negative
        );
        test_instr!(
            |cpu: &mut Cpu| {
                cpu.reg.r[Pc] = 10;
                cpu.reg.r[10] = 10;
            },
            0b010001_01_1_1_010_111, // CMP PC,R10
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 0, 0, 0, 0, 10],
            zero | carry
        );

        // MOV Rd,Rs
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[1] = 15,
            0b010001_10_1_0_001_101, // MOV R13,R1
            [0, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 0, 4],
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[8] = 15,
            0b010001_10_1_1_001_001, // MOV R8,R8
            [0, 0, 0, 0, 0, 0, 0, 0, 15, 0, 0, 0, 0, 0, 0, 4],
        );

        // BX Rs
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[1] = 0b110,
            0b010001_11_1_0_001_101, // BX R1
            [0, 0b110, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0b110 + 4],
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[13] = 0b111,
            0b010001_11_0_1_101_000, // BX R13
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0b111, 0, 0b100 + 8],
        );
    }

    #[test]
    fn execute_thumb6() {
        let mut bus = VecBus(vec![0; 88]);
        bus.write_word(52, 0xdead_beef);
        bus.write_word(84, 0xbead_feed);

        // LDR Rd,[PC,#nn]
        test_instr!(
            &mut bus,
            |_| {},
            0b01001_101_00001100, // LDR R5,[PC,#48]
            [0, 0, 0, 0, 0, 0xdead_beef, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        test_instr!(
            &mut bus,
            |cpu: &mut Cpu| cpu.reg.r[Pc] = 20,
            0b01001_000_00010000, // LDR R0,[PC,#64]
            [0xbead_feed, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 20],
        );
    }

    #[test]
    fn execute_thumb7() {
        let mut bus = VecBus(vec![0; 88]);

        // STR Rd,[Rb,Ro]
        test_instr!(
            &mut bus,
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 0xabcd_ef01;
                cpu.reg.r[1] = 10;
                cpu.reg.r[2] = 5;
            },
            0b0101_00_0_010_001_000, // STR R0,[R1,R2]
            [0xabcd_ef01, 10, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        assert_eq!(0xabcd_ef01, bus.read_word(12));

        test_instr!(
            &mut bus,
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 0x0102_abbc;
                cpu.reg.r[1] = 12;
                cpu.reg.r[2] = 4;
            },
            0b0101_00_0_010_001_000, // STR R0,[R1,R2]
            [0x0102_abbc, 12, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        assert_eq!(0x0102_abbc, bus.read_word(16));

        // STRB Rd,[Rb,Ro]
        test_instr!(
            &mut bus,
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 0xabab;
                cpu.reg.r[1] = 10;
                cpu.reg.r[2] = 9;
            },
            0b0101_01_0_010_001_000, // STRB R0,[R1,R2]
            [0xabab, 10, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        assert_eq!(0xab, bus.read_byte(19));
        assert_eq!(0, bus.read_byte(20));

        // LDR Rd,[Rb,Ro]
        test_instr!(
            &mut bus,
            |cpu: &mut Cpu| {
                cpu.reg.r[1] = 7;
                cpu.reg.r[2] = 8;
            },
            0b0101_10_0_010_001_000, // LDR R0,[R1,R2]
            [0xabcd_ef01, 7, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );

        // LDRB Rd,[Rb,Ro]
        test_instr!(
            &mut bus,
            |cpu: &mut Cpu| {
                cpu.reg.r[1] = 2;
                cpu.reg.r[6] = 17;
            },
            0b0101_11_0_110_001_000, // LDRB R0,[R1,R6]
            [0xab, 2, 0, 0, 0, 0, 17, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
    }

    #[test]
    fn execute_thumb8() {
        let mut bus = VecBus(vec![0; 22]);
        bus.write_byte(0, 0b0111_1110);
        bus.write_byte(18, 1 << 7);
        bus.write_byte(21, !1);

        // STRH Rd,[Rb,Ro]
        test_instr!(
            &mut bus,
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 0xabcd_ef01;
                cpu.reg.r[1] = 10;
                cpu.reg.r[2] = 5;
            },
            0b0101_00_1_010_001_000, // STRH R0,[R1,R2]
            [0xabcd_ef01, 10, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        assert_eq!(0xef01, bus.read_hword(14));
        assert_eq!(0, bus.read_hword(16));

        // LDSB Rd,[Rb,Ro]
        #[rustfmt::skip]
        test_instr!(
            &mut bus,
            |cpu: &mut Cpu| {
                cpu.reg.r[1] = 20;
                cpu.reg.r[2] = 1;
            },
            0b0101_01_1_010_001_000, // LDSB R0,[R1,R2]
            [i32::from(!1u8) as _, 20, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        test_instr!(
            &mut bus,
            |_| {},
            0b0101_01_1_010_001_000, // LDSB R0,[R1,R2]
            [0b0111_1110, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );

        // LDRH Rd,[Rb,Ro]
        test_instr!(
            &mut bus,
            |cpu: &mut Cpu| {
                cpu.reg.r[1] = 13;
                cpu.reg.r[2] = 1;
            },
            0b0101_10_1_010_001_000, // LDRH R0,[R1,R2]
            [0xef01, 13, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );

        // LDSH Rd,[Rb,Ro]
        #[rustfmt::skip]
        test_instr!(
            &mut bus,
            |cpu: &mut Cpu| {
                cpu.reg.r[1] = 2;
                cpu.reg.r[2] = 17;
            },
            0b0101_11_1_010_001_000, // LDSH R0,[R1,R2]
            [i32::from(1 << 7) as _, 2, 17, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
    }

    #[test]
    fn execute_thumb9() {
        let mut bus = VecBus(vec![0; 40]);

        // STR Rd,[Rb,#nn]
        test_instr!(
            &mut bus,
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 0xabcd_ef01;
                cpu.reg.r[1] = 10;
            },
            0b011_00_00110_001_000, // STR R0,[R1,#24]
            [0xabcd_ef01, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        assert_eq!(0xabcd_ef01, bus.read_word(32));

        // LDR Rd,[Rb,#nn]
        test_instr!(
            &mut bus,
            |cpu: &mut Cpu| cpu.reg.r[1] = 8,
            0b011_01_00110_001_000, // LDR R0,[R1,#24]
            [0xabcd_ef01, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );

        // STRB Rd,[Rb,#nn]
        test_instr!(
            &mut bus,
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 0xabcd_ef01;
                cpu.reg.r[1] = 10;
            },
            0b011_10_00110_001_000, // STRB R0,[R1,#6]
            [0xabcd_ef01, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        assert_eq!(0x01, bus.read_byte(16));

        // LDRB Rd,[Rb,#nn]
        test_instr!(
            &mut bus,
            |cpu: &mut Cpu| cpu.reg.r[1] = 10,
            0b011_11_00110_001_000, // LDRB R0,[R1,#6]
            [0x01, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
    }

    #[test]
    fn execute_thumb10() {
        let mut bus = VecBus(vec![0; 40]);

        // STRH Rd,[Rb,#nn]
        test_instr!(
            &mut bus,
            |cpu: &mut Cpu| {
                cpu.reg.r[0] = 0xabcd_ef01;
                cpu.reg.r[1] = 10;
            },
            0b1000_0_00101_001_000, // STRH R0,[R1,#10]
            [0xabcd_ef01, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        assert_eq!(0xef01, bus.read_hword(20));

        // LDRH Rd,[Rb,#nn]
        test_instr!(
            &mut bus,
            |cpu: &mut Cpu| cpu.reg.r[1] = 9,
            0b1000_1_00110_001_000, // LDRH R0,[R1,#12]
            [0xef01, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
    }

    #[test]
    fn execute_thumb11() {
        let mut bus = VecBus(vec![0; 40]);

        // STR Rd,[SP,#nn]
        test_instr!(
            &mut bus,
            |cpu: &mut Cpu| {
                cpu.reg.r[Sp] = 8;
                cpu.reg.r[0] = 0xabcd_ef01;
            },
            0b1001_0_000_00000010, // STR R0,[SP,#8]
            [0xabcd_ef01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 4],
        );
        assert_eq!(0xabcd_ef01, bus.read_word(16));

        // LDR Rd,[SP,#nn]
        test_instr!(
            &mut bus,
            |cpu: &mut Cpu| cpu.reg.r[Sp] = 1,
            0b1001_1_000_00000100, // LDR R0,[SP,#16]
            [0xabcd_ef01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 4],
        );
    }

    #[test]
    fn execute_thumb12() {
        // ADD Rd,[PC,#nn]
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[Pc] = 20,
            0b1010_0_000_11001000, // ADD R0,[PC,#200]
            [220, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 20],
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[Pc] = 0,
            0b1010_0_000_00000000, // ADD R0,[PC,#0]
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );

        // ADD Rd,[SP,#nn]
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[Sp] = 40,
            0b1010_1_000_11001000, // ADD R0,[SP,#200]
            [240, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 40, 0, 4],
        );
        test_instr!(
            0b1010_1_000_00000000, // ADD R0,[SP,#0]
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
    }

    #[test]
    fn execute_thumb13() {
        // ADD SP,#nn
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[Sp] = 1,
            0b10110000_0_0110010, // ADD SP,#200
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 201, 0, 4],
        );
        test_instr!(
            0b10110000_0_0000000, // ADD SP,#0
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );

        // SUB SP,#nn
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[Sp] = 200,
            0b10110000_1_0110010, // SUB SP,#200
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
        );
        test_instr!(
            |cpu: &mut Cpu| cpu.reg.r[Sp] = 50,
            0b10110000_1_0110010, // SUB SP,#200
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, u32::MAX - 149, 0, 4],
        );
    }

    #[test]
    fn execute_thumb14() {
        let mut bus = VecBus(vec![0; 40]);

        // PUSH {Rlist}{LR}
        #[rustfmt::skip]
        test_instr!(
            &mut bus,
            |cpu: &mut Cpu| {
                cpu.reg.r[Sp] = 40;
                cpu.reg.r[0] = 0xabcd;
                cpu.reg.r[3] = 0xfefe_0001;
                cpu.reg.r[7] = 42;
            },
            0b1011_0_10_0_10001001, // PUSH {R0,R3,R7}
            [0xabcd, 0, 0, 0xfefe_0001, 0, 0, 0, 42, 0, 0, 0, 0, 0, 28, 0, 4],
        );
        assert_eq!(42, bus.read_word(36));
        assert_eq!(0xfefe_0001, bus.read_word(32));
        assert_eq!(0xabcd, bus.read_word(28));

        #[rustfmt::skip]
        test_instr!(
            &mut bus,
            |cpu: &mut Cpu| {
                cpu.reg.r[Sp] = 28;
                cpu.reg.r[1] = 0b1010;
                cpu.reg.r[Lr] = 40;
            },
            0b1011_0_10_1_00000010, // PUSH {R1,LR}
            [0, 0b1010, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 20, 40, 4],
        );
        assert_eq!(40, bus.read_word(24));
        assert_eq!(0b1010, bus.read_word(20));

        // POP {Rlist}{PC}
        #[rustfmt::skip]
        test_instr!(
            &mut bus,
            |cpu: &mut Cpu| cpu.reg.r[Sp] = 20,
            0b1011_1_10_1_00000001, // POP {R1,PC}
            [0b1010, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 28, 0, 44],
        );
        #[rustfmt::skip]
        test_instr!(
            &mut bus,
            |cpu: &mut Cpu| cpu.reg.r[Sp] = 28,
            0b1011_1_10_0_10001001, // POP {R0,R3,R7}
            [0xabcd, 0, 0, 0xfefe_0001, 0, 0, 0, 42, 0, 0, 0, 0, 0, 40, 0, 4],
        );
    }
}
