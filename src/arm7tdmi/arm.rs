use intbits::Bits;

use crate::{
    arm7tdmi::{reg::OperationState, Exception},
    bus::Bus,
    sign_extend,
};

use super::{reg::PC_INDEX, Cpu};

fn r_index(instr: u32, pos: u8) -> usize {
    instr.bits(pos..(pos + 4)) as _
}

impl Cpu {
    pub(crate) fn execute_arm(&mut self, bus: &mut impl Bus, instr: u32) {
        assert!(self.reg.cpsr.state == OperationState::Arm);

        #[allow(clippy::cast_possible_truncation)]
        if !self.meets_condition(instr.bits(28..) as u8) {
            return; // TODO: 1S cycle anyway
        }

        match (
            instr.bits(26..28),
            instr.bits(25..28),
            instr.bits(24..28),
            instr.bits(8..28),
        ) {
            (_, 0b101, _, _) => self.execute_arm_b_bl(bus, instr),
            (_, _, _, 0b0001_0010_1111_1111_1111) => self.execute_arm_bx(bus, instr),
            // TODO: 2S+1N, also what happens if 0b0001? BKPT does apply to ARMv4
            (_, _, 0b1111, _) => self.enter_exception(bus, Exception::SoftwareInterrupt),
            // TODO: 2S+1N+1I
            (_, 0b011, _, _) => self.enter_exception(bus, Exception::UndefinedInstr),
            (0b00, _, _, _) => self.execute_arm_alu(bus, instr),
            _ => todo!(),
        }
    }

    /// Branch and branch with link.
    fn execute_arm_b_bl(&mut self, bus: &impl Bus, instr: u32) {
        // TODO: 2S+1N
        // {cond} label
        let addr_offset = 4 * sign_extend!(i32, instr.bits(..24), 24);
        if instr.bit(24) {
            // BL
            self.execute_arm_bl(bus, addr_offset);
        } else {
            // B
            self.execute_branch(bus, self.reg.r[PC_INDEX], addr_offset);
        }
    }

    /// Branch and exchange.
    fn execute_arm_bx(&mut self, bus: &impl Bus, instr: u32) {
        // TODO: 2S+1N
        // TODO: bits 4-7 should be 0b0001, but what happens if they're not?
        // {cond} Rn
        self.execute_bx(bus, self.reg.r[r_index(instr, 0)]);
    }

    /// ALU operations.
    fn execute_arm_alu(&mut self, bus: &impl Bus, instr: u32) {
        // TODO: (1+p)S+rI+pN. Whereas r=1 if I=0 and R=1 (ie. shift by register); otherwise r=0.
        //       And p=1 if Rd=R15; otherwise p=0.
        // TODO: do these instructions act weird when, e.g, reserved bits are set?
        let update_cond = instr.bit(20);
        let r_value1 = r_index(instr, 16);
        let mut value1 = self.reg.r[r_value1];
        let r_dst = r_index(instr, 12);

        let value2 = if instr.bit(25) {
            // Operand 2 is an ROR'd immediate value.
            #[allow(clippy::cast_possible_truncation)]
            self.execute_ror(
                update_cond,
                false,
                instr.bits(0..8),
                2 * (instr.bits(8..12) as u8),
            )
        } else {
            // Operand 2 is from a register.
            let offset_from_reg = instr.bit(4);

            #[allow(clippy::cast_possible_truncation)]
            let offset = if offset_from_reg {
                self.reg.r[r_index(instr, 8)].bits(..8) as u8
            } else {
                instr.bits(7..12) as u8
            };

            let r_value2 = r_index(instr, 0);
            let mut value2 = self.reg.r[r_value2];

            // If PC is Rn or Rm, it is read as PC+12, not PC+8 (so an extra instr ahead).
            if offset_from_reg {
                if r_value1 == PC_INDEX {
                    value1 = value1.wrapping_add(self.reg.cpsr.state.instr_size());
                }
                if r_value2 == PC_INDEX {
                    value2 = value2.wrapping_add(self.reg.cpsr.state.instr_size());
                }
            }

            match instr.bits(5..7) {
                // LSL
                0 => self.execute_lsl(update_cond, value2, offset),
                // LSR
                1 => self.execute_lsr(update_cond, !offset_from_reg, value2, offset),
                // ASR
                2 => self.execute_asr(update_cond, !offset_from_reg, value2, offset),
                // ROR
                3 => self.execute_ror(update_cond, !offset_from_reg, value2, offset),
                _ => unreachable!(),
            }
        };

        let op = instr.bits(21..25);

        match op {
            // AND{cond}{S} Rd,Rn,Op2
            0 => self.reg.r[r_dst] = self.execute_and_tst(update_cond, value1, value2),
            // EOR{cond}{S} Rd,Rn,Op2
            1 => self.reg.r[r_dst] = self.execute_eor_teq(update_cond, value1, value2),
            // SUB{cond}{S} Rd,Rn,Op2
            2 => self.reg.r[r_dst] = self.execute_sub_cmp(update_cond, value1, value2),
            // RSB{cond}{S} Rd,Rn,Op2
            3 => self.reg.r[r_dst] = self.execute_sub_cmp(update_cond, value2, value1),
            // ADD{cond}{S} Rd,Rn,Op2
            4 => self.reg.r[r_dst] = self.execute_add_cmn(update_cond, value1, value2),
            // ADC{cond}{S} Rd,Rn,Op2
            5 => self.reg.r[r_dst] = self.execute_adc(update_cond, value1, value2),
            // SBC{cond}{S} Rd,Rn,Op2
            6 => self.reg.r[r_dst] = self.execute_sbc(update_cond, value1, value2),
            // RSC{cond}{S} Rd,Rn,Op2
            7 => self.reg.r[r_dst] = self.execute_sbc(update_cond, value2, value1),
            // TST{cond}{P} Rn,Op2
            8 => {
                self.execute_and_tst(true, value1, value2);
            }
            // TEQ{cond}{P} Rn,Op2
            9 => {
                self.execute_eor_teq(true, value1, value2);
            }
            // CMP{cond}{P} Rn,Op2
            10 => {
                self.execute_sub_cmp(true, value1, value2);
            }
            // CMN{cond}{P} Rn,Op2
            11 => {
                self.execute_add_cmn(true, value1, value2);
            }
            // ORR{cond}{S} Rd,Rn,Op2
            12 => self.reg.r[r_dst] = self.execute_orr(update_cond, value1, value2),
            // MOV{cond}{S} Rd,Op2
            13 => self.reg.r[r_dst] = self.execute_mov(update_cond, value2),
            // BIC{cond}{S} Rd,Rn,Op2
            14 => self.reg.r[r_dst] = self.execute_bic(update_cond, value1, value2),
            // MVN{cond}{S} Rd,Op2
            15 => self.reg.r[r_dst] = self.execute_mvn(update_cond, value2),
            _ => unreachable!(),
        }

        if !(8..=11).contains(&op) && r_dst == PC_INDEX {
            self.reload_pipeline(bus);
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

    use intbits::Bits;

    use crate::arm7tdmi::{op::tests::InstrTest, reg::LR_INDEX};

    #[test]
    fn execute_arm_cond_branch() {
        // B{cond} label; also test a few ARM {cond}itions here.
        InstrTest::new_arm(0b1001_101_0_010000000000000000000001) // LS #1000004h
            .setup(&|cpu| cpu.reg.cpsr.zero = true)
            .assert_r(PC_INDEX, 8 + 0x100_0004 + 8)
            .assert_zero()
            .run();

        InstrTest::new_arm(0b1001_101_0_111111111111111111111111) // LS #(-4)
            .assert_r(PC_INDEX, 8 - 4 + 8)
            .run();

        InstrTest::new_arm(0b1001_101_0_111111111111111111111111) // LS #(-4)
            .setup(&|cpu| {
                cpu.reg.cpsr.carry = true;
                cpu.reg.cpsr.zero = false;
            })
            .assert_carry()
            .run();

        // BL{cond} label
        InstrTest::new_arm(0b0110_101_1_010000000000000000000001) // VS #1000004h
            .setup(&|cpu| cpu.reg.cpsr.overflow = true)
            .assert_r(LR_INDEX, 4)
            .assert_r(PC_INDEX, 8 + 0x100_0004 + 8)
            .assert_overflow()
            .run();

        InstrTest::new_arm(0b0110_101_1_010000000000000000000001) // VS #1000004h
            .run();

        // BX{cond} Rn
        let cpu = InstrTest::new_arm(0b1110_00010010111111111111_0001_1011) // AL R11
            .setup(&|cpu| cpu.reg.r[11] = 0b1111)
            .assert_r(11, 0b1111)
            .assert_r(PC_INDEX, 0b1110 + 4)
            .run();

        assert_eq!(cpu.reg.cpsr.state, OperationState::Thumb);

        let cpu = InstrTest::new_arm(0b1110_00010010111111111111_0001_1011) // AL R11
            .setup(&|cpu| cpu.reg.r[11] = 0b1110)
            .assert_r(11, 0b1110)
            .assert_r(PC_INDEX, 0b1100 + 8)
            .run();

        assert_eq!(cpu.reg.cpsr.state, OperationState::Arm);

        let cpu = InstrTest::new_arm(0b1110_00010010111111111111_0001_1111) // AL R15
            .assert_r(PC_INDEX, 8 + 8)
            .run();

        assert_eq!(cpu.reg.cpsr.state, OperationState::Arm);

        // SWI{cond} nn
        InstrTest::new_arm(0b1110_1111_001011111111111100011110) // AL
            .setup(&|cpu| cpu.reg.cpsr.irq_disabled = false)
            .assert_r(LR_INDEX, 8 - 4)
            .assert_r(PC_INDEX, 0x08 + 8)
            .run();

        // Undefined
        InstrTest::new_arm(0b1110_011_01010101010101010101_1_1010) // AL
            .setup(&|cpu| cpu.reg.cpsr.irq_disabled = false)
            .assert_r(LR_INDEX, 8 - 4)
            .assert_r(PC_INDEX, 0x04 + 8)
            .run();
    }

    #[test]
    fn execute_arm_alu_and_args() {
        // AND{cond}{S} Rd,Rn,Op2; mostly test argument decoding and handling here.
        // AL S R14,R0,#10101010b
        InstrTest::new_arm(0b1110_00_1_0000_1_0000_1110_0000_10101010)
            .setup(&|cpu| cpu.reg.r[0] = 0b1100_0011)
            .assert_r(0, 0b1100_0011)
            .assert_r(14, 0b1000_0010)
            .run();

        // AL S R14,R0,#0
        InstrTest::new_arm(0b1110_00_1_0000_1_0000_1110_0000_00000000)
            .setup(&|cpu| cpu.reg.r[0] = 0b1100_0011)
            .assert_r(0, 0b1100_0011)
            .assert_zero()
            .run();

        // AL R14,R0,#0
        InstrTest::new_arm(0b1110_00_1_0000_0_0000_1110_0000_00000000)
            .setup(&|cpu| cpu.reg.r[0] = 0b1100_0011)
            .assert_r(0, 0b1100_0011)
            .run();

        // AL S R14,R0,#11100001b,ROR#6
        InstrTest::new_arm(0b1110_00_1_0000_1_0000_1110_0011_11100001)
            .setup(&|cpu| cpu.reg.r[0] = u32::MAX)
            .assert_r(0, u32::MAX)
            .assert_r(14, 0b11.with_bits(26.., 0b10_0001))
            .assert_negative()
            .assert_carry()
            .run();

        // AL R14,R0,#11100001b,ROR#6
        InstrTest::new_arm(0b1110_00_1_0000_0_0000_1110_0011_11100001)
            .setup(&|cpu| cpu.reg.r[0] = u32::MAX)
            .assert_r(0, u32::MAX)
            .assert_r(14, 0b11.with_bits(26.., 0b10_0001))
            .run();

        // AL S R3,R15,#11111111b
        InstrTest::new_arm(0b1110_00_1_0000_1_1111_0011_0000_11111111)
            .assert_r(3, 8)
            .run();

        // AL S R15,R0,#1011b
        InstrTest::new_arm(0b1110_00_1_0000_1_0000_1111_0000_00001011)
            .setup(&|cpu| cpu.reg.r[0] = u32::MAX)
            .assert_r(0, u32::MAX)
            .assert_r(PC_INDEX, 8 + 8)
            .run();

        // AL S R9,R0,R11,LSL#30
        InstrTest::new_arm(0b1110_00_0_0000_1_0000_1001_11110_00_0_1011)
            .setup(&|cpu| {
                cpu.reg.r[0] = 0b111 << 29;
                cpu.reg.r[11] = 0b111;
            })
            .assert_r(0, 0b111 << 29)
            .assert_r(9, 0b11 << 30)
            .assert_r(11, 0b111)
            .assert_carry()
            .assert_negative()
            .run();

        // AL S R9,R0,R11,LSL#0
        InstrTest::new_arm(0b1110_00_0_0000_1_0000_1001_00000_00_0_1011)
            .setup(&|cpu| {
                cpu.reg.r[0] = 0b111 << 29;
                cpu.reg.r[11] = 0b111;
            })
            .assert_r(0, 0b111 << 29)
            .assert_r(11, 0b111)
            .assert_zero()
            .run();

        // AL S R9,R0,R11,LSR#2
        InstrTest::new_arm(0b1110_00_0_0000_1_0000_1001_00010_01_0_1011)
            .setup(&|cpu| {
                cpu.reg.r[0] = 0b111 << 29;
                cpu.reg.r[11] = u32::MAX;
            })
            .assert_r(0, 0b111 << 29)
            .assert_r(9, 0b001 << 29)
            .assert_r(11, u32::MAX)
            .assert_carry()
            .run();

        // AL S R9,R0,R11,LSR#0
        InstrTest::new_arm(0b1110_00_0_0000_1_0000_1001_00000_01_0_1011)
            .setup(&|cpu| {
                cpu.reg.r[0] = 0b111 << 29;
                cpu.reg.r[11] = u32::MAX;
            })
            .assert_r(0, 0b111 << 29)
            .assert_r(11, u32::MAX)
            .assert_carry()
            .assert_zero()
            .run();

        // AL S R9,R0,R11,LSR#0
        InstrTest::new_arm(0b1110_00_0_0000_1_0000_1001_00000_01_0_1011)
            .setup(&|cpu| {
                cpu.reg.r[0] = 0b111 << 29;
                cpu.reg.r[11] = u32::MAX.with_bit(31, false);
            })
            .assert_r(0, 0b111 << 29)
            .assert_r(11, u32::MAX.with_bit(31, false))
            .assert_zero()
            .run();

        // AL S R9,R0,R11,ASR#2
        InstrTest::new_arm(0b1110_00_0_0000_1_0000_1001_00010_10_0_1011)
            .setup(&|cpu| {
                cpu.reg.r[0] = 0b111 << 29;
                cpu.reg.r[11] = u32::MAX;
            })
            .assert_r(0, 0b111 << 29)
            .assert_r(9, 0b111 << 29)
            .assert_r(11, u32::MAX)
            .assert_carry()
            .assert_negative()
            .run();

        // AL S R9,R0,R11,ASR#0
        InstrTest::new_arm(0b1110_00_0_0000_1_0000_1001_00000_10_0_1011)
            .setup(&|cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[11] = 1 << 31;
            })
            .assert_r(0, u32::MAX)
            .assert_r(9, u32::MAX)
            .assert_r(11, 1 << 31)
            .assert_carry()
            .assert_negative()
            .run();

        // AL S R9,R0,R11,ROR#3
        InstrTest::new_arm(0b1110_00_0_0000_1_0000_1001_00011_11_0_1011)
            .setup(&|cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[11] = 0b101_0101;
            })
            .assert_r(0, u32::MAX)
            .assert_r(9, 0b1010.with_bits(29.., 0b101))
            .assert_r(11, 0b101_0101)
            .assert_carry()
            .assert_negative()
            .run();

        // AL S R9,R0,R11,ROR#0
        InstrTest::new_arm(0b1110_00_0_0000_1_0000_1001_00000_11_0_1011)
            .setup(&|cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[11] = 0b101_0101;
            })
            .assert_r(0, u32::MAX)
            .assert_r(9, 0b10_1010)
            .assert_r(11, 0b101_0101)
            .assert_carry()
            .run();

        // AL S R9,R0,R11,ROR#0
        InstrTest::new_arm(0b1110_00_0_0000_1_0000_1001_00000_11_0_1011)
            .setup(&|cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[11] = 0b101_0101;
                cpu.reg.cpsr.carry = true;
            })
            .assert_r(0, u32::MAX)
            .assert_r(9, 0b10_1010.with_bit(31, true))
            .assert_r(11, 0b101_0101)
            .assert_carry()
            .assert_negative()
            .run();

        // AL S R9,R0,R15,LSL#1
        InstrTest::new_arm(0b1110_00_0_0000_1_0000_1001_00001_00_0_1111)
            .setup(&|cpu| cpu.reg.r[0] = u32::MAX)
            .assert_r(0, u32::MAX)
            .assert_r(9, 8 << 1)
            .run();

        // AL S R9,R15,R0,LSL#0
        InstrTest::new_arm(0b1110_00_0_0000_1_1111_1001_00000_00_0_0000)
            .setup(&|cpu| cpu.reg.r[0] = u32::MAX)
            .assert_r(0, u32::MAX)
            .assert_r(9, 8)
            .run();

        // AL S R9,R0,R5,LSL R3 (only lo byte of R3 is used, so shift should be 1)
        InstrTest::new_arm(0b1110_00_0_0000_1_0000_1001_0011_0_00_1_0101)
            .setup(&|cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[3] = u32::MAX.with_bits(..8, 1);
                cpu.reg.r[5] = u32::MAX;
            })
            .assert_r(0, u32::MAX)
            .assert_r(3, u32::MAX.with_bits(..8, 1))
            .assert_r(5, u32::MAX)
            .assert_r(9, u32::MAX.with_bit(0, false))
            .assert_carry()
            .assert_negative()
            .run();

        // For the next few tests, PC should read an extra instr ahead; PC+12 in total.
        // AL S R9,R15,R5,LSL R3
        InstrTest::new_arm(0b1110_00_0_0000_1_1111_1001_0011_0_00_1_0101)
            .setup(&|cpu| cpu.reg.r[5] = u32::MAX)
            .assert_r(5, u32::MAX)
            .assert_r(9, 8 + 4)
            .run();

        // AL S R9,R5,R15,LSL R14
        InstrTest::new_arm(0b1110_00_0_0000_1_0101_1001_1110_0_00_1_1111)
            .setup(&|cpu| cpu.reg.r[5] = u32::MAX)
            .assert_r(5, u32::MAX)
            .assert_r(9, 8 + 4)
            .run();

        // AL S R9,R15,R15,LSL R14
        InstrTest::new_arm(0b1110_00_0_0000_1_1111_1001_1110_0_00_1_1111)
            .assert_r(9, 8 + 4)
            .run();
    }

    #[test]
    fn execute_arm_alu_ops() {
        // AND plus argument handling is already tested in execute_arm_alu_and_args.
        // We'll test the other ops here with and without the condition bit set
        // (except TST, TEQ, CMP and CMN).

        // EOR{cond}{S} Rd,Rn,Op2
        // AL S R14,R0,#10101010b
        InstrTest::new_arm(0b1110_00_1_0001_1_0000_1110_0000_10101010)
            .setup(&|cpu| cpu.reg.r[0] = 0b1100_0011.with_bit(31, true))
            .assert_r(0, 0b1100_0011.with_bit(31, true))
            .assert_r(14, 0b110_1001.with_bit(31, true))
            .assert_negative()
            .run();

        // AL R14,R0,#10101010b
        InstrTest::new_arm(0b1110_00_1_0001_0_0000_1110_0000_10101010)
            .setup(&|cpu| cpu.reg.r[0] = 0b1100_0011.with_bit(31, true))
            .assert_r(0, 0b1100_0011.with_bit(31, true))
            .assert_r(14, 0b110_1001.with_bit(31, true))
            .run();

        // SUB{cond}{S} Rd,Rn,Op2
        // AL S R14,R0,#20
        InstrTest::new_arm(0b1110_00_1_0010_1_0000_1110_0000_00010100)
            .setup(&|cpu| cpu.reg.r[0] = 15)
            .assert_r(0, 15)
            .assert_r(14, -5 as _)
            .assert_negative()
            .run();

        // AL R14,R0,#20
        InstrTest::new_arm(0b1110_00_1_0010_0_0000_1110_0000_00010100)
            .setup(&|cpu| cpu.reg.r[0] = 15)
            .assert_r(0, 15)
            .assert_r(14, -5 as _)
            .run();

        // RSB{cond}{S} Rd,Rn,Op2
        // AL S R14,R0,#20
        InstrTest::new_arm(0b1110_00_1_0011_1_0000_1110_0000_00010100)
            .setup(&|cpu| cpu.reg.r[0] = 15)
            .assert_r(0, 15)
            .assert_r(14, 5)
            .assert_carry()
            .run();

        // AL R14,R0,#20
        InstrTest::new_arm(0b1110_00_1_0011_0_0000_1110_0000_00010100)
            .setup(&|cpu| cpu.reg.r[0] = 15)
            .assert_r(0, 15)
            .assert_r(14, 5)
            .run();

        // ADD{cond}{S} Rd,Rn,Op2
        // AL S R14,R0,#3
        InstrTest::new_arm(0b1110_00_1_0100_1_0000_1110_0000_00000011)
            .setup(&|cpu| cpu.reg.r[0] = -15 as _)
            .assert_r(0, -15 as _)
            .assert_r(14, -12 as _)
            .assert_negative()
            .run();

        // AL R14,R0,#3
        InstrTest::new_arm(0b1110_00_1_0100_0_0000_1110_0000_00000011)
            .setup(&|cpu| cpu.reg.r[0] = -15 as _)
            .assert_r(0, -15 as _)
            .assert_r(14, -12 as _)
            .run();

        // ADC{cond}{S} Rd,Rn,Op2
        // AL S R14,R0,#3
        InstrTest::new_arm(0b1110_00_1_0101_1_0000_1110_0000_00000011)
            .setup(&|cpu| {
                cpu.reg.cpsr.carry = true;
                cpu.reg.r[0] = -15 as _;
            })
            .assert_r(0, -15 as _)
            .assert_r(14, -11 as _)
            .assert_negative()
            .run();

        // AL R14,R0,#3
        InstrTest::new_arm(0b1110_00_1_0101_0_0000_1110_0000_00000011)
            .setup(&|cpu| {
                cpu.reg.cpsr.carry = true;
                cpu.reg.r[0] = -15 as _;
            })
            .assert_r(0, -15 as _)
            .assert_r(14, -11 as _)
            .assert_carry()
            .run();

        // SBC{cond}{S} Rd,Rn,Op2
        // AL S R14,R0,#20
        InstrTest::new_arm(0b1110_00_1_0110_1_0000_1110_0000_00010100)
            .setup(&|cpu| cpu.reg.r[0] = 15)
            .assert_r(0, 15)
            .assert_r(14, -6 as _)
            .assert_negative()
            .assert_carry()
            .run();

        // AL R14,R0,#20
        InstrTest::new_arm(0b1110_00_1_0110_0_0000_1110_0000_00010100)
            .setup(&|cpu| cpu.reg.r[0] = 15)
            .assert_r(0, 15)
            .assert_r(14, -6 as _)
            .run();

        // RSC{cond}{S} Rd,Rn,Op2
        // AL S R14,R0,#20
        InstrTest::new_arm(0b1110_00_1_0111_1_0000_1110_0000_00010100)
            .setup(&|cpu| cpu.reg.r[0] = 15)
            .assert_r(0, 15)
            .assert_r(14, 4)
            .assert_carry()
            .run();

        // AL R14,R0,#20
        InstrTest::new_arm(0b1110_00_1_0111_0_0000_1110_0000_00010100)
            .setup(&|cpu| cpu.reg.r[0] = 15)
            .assert_r(0, 15)
            .assert_r(14, 4)
            .run();

        // TST{cond}{P} Rn,Op2
        // AL R0,#10101010b
        InstrTest::new_arm(0b1110_00_1_1000_1_0000_1111_0000_10101010)
            .setup(&|cpu| cpu.reg.r[0] = 1)
            .assert_r(0, 1)
            .assert_zero()
            .run();

        // AL R0,#10101010b
        InstrTest::new_arm(0b1110_00_1_1000_1_0000_0000_0000_10101010)
            .setup(&|cpu| cpu.reg.r[0] = 0b10)
            .assert_r(0, 0b10)
            .run();

        // TEQ{cond}{P} Rn,Op2
        // AL R0,#10101010b
        InstrTest::new_arm(0b1110_00_1_1001_1_0000_0000_0000_10101010)
            .setup(&|cpu| cpu.reg.r[0] = 0b1010_1011)
            .assert_r(0, 0b1010_1011)
            .run();

        // AL R0,#10101010b
        InstrTest::new_arm(0b1110_00_1_1001_1_0000_1111_0000_10101010)
            .setup(&|cpu| cpu.reg.r[0] = 0b1010_1010)
            .assert_r(0, 0b1010_1010)
            .assert_zero()
            .run();

        // CMP{cond}{P} Rn,Op2
        // AL S R0,#20
        InstrTest::new_arm(0b1110_00_1_1010_1_0000_1111_0000_00010100)
            .setup(&|cpu| cpu.reg.r[0] = 15)
            .assert_r(0, 15)
            .assert_negative()
            .run();

        // AL R0,#20
        InstrTest::new_arm(0b1110_00_1_1010_1_0000_0000_0000_00010100)
            .setup(&|cpu| cpu.reg.r[0] = 20)
            .assert_r(0, 20)
            .assert_zero()
            .assert_carry()
            .run();

        // CMN{cond}{P} Rn,Op2
        // AL R0,#3
        InstrTest::new_arm(0b1110_00_1_1011_1_0000_0000_0000_00000011)
            .setup(&|cpu| cpu.reg.r[0] = -15 as _)
            .assert_r(0, -15 as _)
            .assert_negative()
            .run();

        // AL R0,#15
        InstrTest::new_arm(0b1110_00_1_1011_1_0000_1111_0000_00001111)
            .setup(&|cpu| cpu.reg.r[0] = -15 as _)
            .assert_r(0, -15 as _)
            .assert_zero()
            .assert_carry()
            .run();

        // ORR{cond}{S} Rd,Rn,Op2
        // AL S R15,R0,#10101010b
        InstrTest::new_arm(0b1110_00_1_1100_1_0000_1111_0000_10101010)
            .setup(&|cpu| cpu.reg.r[0] = 0b1100_0011.with_bit(31, true))
            .assert_r(0, 0b1100_0011.with_bit(31, true))
            .assert_r(PC_INDEX, 0b1110_1000.with_bit(31, true) + 8)
            .assert_negative()
            .run();

        // AL R14,R0,#10101010b
        InstrTest::new_arm(0b1110_00_1_1100_0_0000_1110_0000_10101010)
            .setup(&|cpu| cpu.reg.r[0] = 0b1100_0011.with_bit(31, true))
            .assert_r(0, 0b1100_0011.with_bit(31, true))
            .assert_r(14, 0b1110_1011.with_bit(31, true))
            .run();

        // MOV{cond}{S} Rd,Op2
        // AL S R14,#0
        InstrTest::new_arm(0b1110_00_1_1101_1_0000_1110_0000_00000000)
            .setup(&|cpu| cpu.reg.r[14] = 1337)
            .assert_zero()
            .run();

        // AL R14,#0
        InstrTest::new_arm(0b1110_00_1_1101_0_0000_1110_0000_00000000)
            .setup(&|cpu| cpu.reg.r[14] = 1337)
            .run();

        // BIC{cond}{S} Rd,Rn,Op2
        // AL S R14,R0,#1
        InstrTest::new_arm(0b1110_00_1_1110_1_0000_1110_0000_00000001)
            .setup(&|cpu| cpu.reg.r[0] = 1)
            .assert_r(0, 1)
            .assert_zero()
            .run();

        // AL R14,R0,#1
        InstrTest::new_arm(0b1110_00_1_1110_0_0000_1110_0000_00000001)
            .setup(&|cpu| cpu.reg.r[0] = 1)
            .assert_r(0, 1)
            .run();

        // MVN{cond}{S} Rd,Op2
        // AL S R14,#1
        InstrTest::new_arm(0b1110_00_1_1111_1_0000_1110_0000_00000001)
            .assert_r(14, u32::MAX.with_bit(0, false))
            .assert_negative()
            .run();

        // AL R14,#0
        InstrTest::new_arm(0b1110_00_1_1111_0_0000_1110_0000_00000001)
            .assert_r(14, u32::MAX.with_bit(0, false))
            .run();
    }
}
