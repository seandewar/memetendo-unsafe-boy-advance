use bitmatch::bitmatch;
use intbits::Bits;

use crate::{
    arbitrary_sign_extend,
    arm7tdmi::{
        reg::{OperationMode, OperationState, PC_INDEX},
        Cpu, Exception,
    },
    bus::Bus,
};

fn r_index(instr: u32, pos: u8) -> usize {
    instr.bits(pos..(pos + 4)) as _
}

impl Cpu {
    #[bitmatch]
    pub(in crate::arm7tdmi) fn execute_arm(&mut self, bus: &mut impl Bus, instr: u32) {
        assert!(self.reg.cpsr.state == OperationState::Arm);

        #[allow(clippy::cast_possible_truncation)]
        if !self.meets_condition(instr.bits(28..) as u8) {
            return; // TODO: 1S cycle anyway
        }

        // TODO: 2S+1N for SWI, 2N+1N+1I for undefined exception
        #[bitmatch]
        match instr.bits(..28) {
            "0001_0010_1111_1111_1111_????_????" => self.execute_arm_bx(bus, instr),
            "0000_????_????_????_????_1001_????" => self.execute_arm_multiply(instr),
            "00?1_0??0_????_????_????_????_????" => self.execute_arm_psr_transfer(instr),
            "000?_????_????_????_????_1??1_????" => {
                self.execute_arm_hword_and_signed_transfer(bus, instr);
            }
            "1111_????_????_????_????_????_????" => {
                self.enter_exception(bus, Exception::SoftwareInterrupt);
            }
            "011?_????_????_????_????_???1_????" => {
                self.enter_exception(bus, Exception::UndefinedInstr);
            }
            "101?_????_????_????_????_????_????" => self.execute_arm_b_bl(bus, instr),
            "00??_????_????_????_????_????_????" => self.execute_arm_data_processing(bus, instr),
            "01??_????_????_????_????_????_????" => self.execute_arm_single_transfer(bus, instr),
            _ => self.enter_exception(bus, Exception::UndefinedInstr),
        }
    }

    /// Branch and branch with link.
    fn execute_arm_b_bl(&mut self, bus: &impl Bus, instr: u32) {
        // TODO: 2S+1N
        let addr_offset = 4 * arbitrary_sign_extend!(i32, instr.bits(..24), 24);
        if instr.bit(24) {
            // BL{cond} label
            self.execute_arm_bl(bus, addr_offset);
        } else {
            // B{cond} label
            self.execute_branch(bus, self.reg.r[PC_INDEX], addr_offset);
        }
    }

    /// Branch and exchange.
    fn execute_arm_bx(&mut self, bus: &impl Bus, instr: u32) {
        // TODO: 2S+1N
        // TODO: bits 4-7 should be 0b0001, but what happens if they're not?
        // BX{cond} Rn
        self.execute_bx(bus, self.reg.r[r_index(instr, 0)]);
    }

    /// Data processing operations.
    fn execute_arm_data_processing(&mut self, bus: &impl Bus, instr: u32) {
        // TODO: (1+p)S+rI+pN. Whereas r=1 if I=0 and R=1 (ie. shift by register); otherwise r=0.
        //       And p=1 if Rd=R15; otherwise p=0.
        // TODO: do these instructions act weird when, e.g, reserved bits are set?
        let update_cond = instr.bit(20);
        let r_value1 = r_index(instr, 16);
        let r_dst = r_index(instr, 12);

        let mut value1 = self.reg.r[r_value1];
        let value2 = if instr.bit(25) {
            // Operand 2 is an ROR'd immediate value.
            #[allow(clippy::cast_possible_truncation)]
            self.execute_ror(
                update_cond,
                false,
                instr.bits(..8),
                2 * (instr.bits(8..12) as u8),
            )
        } else {
            // Operand 2 is from a register.
            let offset_from_reg = instr.bit(4);

            #[allow(clippy::cast_possible_truncation)]
            let offset = if offset_from_reg {
                self.reg.r[r_index(instr, 8)] as u8
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

            #[allow(clippy::cast_possible_truncation)]
            self.execute_shift_operand(
                instr.bits(5..7) as _,
                update_cond,
                !offset_from_reg,
                value2,
                offset,
            )
        };

        let op = instr.bits(21..25);

        match op {
            // AND{cond}{S} Rd,Rn,Op2
            0 => self.reg.r[r_dst] = self.execute_and(update_cond, value1, value2),
            // EOR{cond}{S} Rd,Rn,Op2
            1 => self.reg.r[r_dst] = self.execute_eor(update_cond, value1, value2),
            // SUB{cond}{S} Rd,Rn,Op2
            2 => self.reg.r[r_dst] = self.execute_sub(update_cond, value1, value2),
            // RSB{cond}{S} Rd,Rn,Op2
            3 => self.reg.r[r_dst] = self.execute_sub(update_cond, value2, value1),
            // ADD{cond}{S} Rd,Rn,Op2
            4 => self.reg.r[r_dst] = self.execute_add(update_cond, value1, value2),
            // ADC{cond}{S} Rd,Rn,Op2
            5 => self.reg.r[r_dst] = self.execute_adc(update_cond, value1, value2),
            // SBC{cond}{S} Rd,Rn,Op2
            6 => self.reg.r[r_dst] = self.execute_sbc(update_cond, value1, value2),
            // RSC{cond}{S} Rd,Rn,Op2
            7 => self.reg.r[r_dst] = self.execute_sbc(update_cond, value2, value1),
            // TST{cond} Rn,Op2
            8 => {
                self.execute_and(true, value1, value2);
            }
            // TEQ{cond} Rn,Op2
            9 => {
                self.execute_eor(true, value1, value2);
            }
            // CMP{cond} Rn,Op2
            10 => {
                self.execute_sub(true, value1, value2);
            }
            // CMN{cond} Rn,Op2
            11 => {
                self.execute_add(true, value1, value2);
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

    /// Multiply and multiply-accumulate.
    fn execute_arm_multiply(&mut self, instr: u32) {
        // TODO: Cycle counting -- it's complicated; see GBATEK.
        // TODO: There are some restrictions; what if they're violated?
        //       Right now, due to how our opcode parsing is implemented, using unused opcodes may
        //       be interpreted as valid opcodes; we'll just call it undefined behaviour. :-)
        let update_cond = instr.bit(20);
        let r_dst_or_hi = r_index(instr, 16);
        let r_accum_or_lo = r_index(instr, 12);

        let value1 = self.reg.r[r_index(instr, 0)];
        let value2 = self.reg.r[r_index(instr, 8)];
        let accum1 = self.reg.r[r_accum_or_lo];
        let accum2 = self.reg.r[r_dst_or_hi];

        #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
        if instr.bit(23) {
            // 64-bit result written to RdHiLo.
            let accum_dword = u64::from(accum1).with_bits(32.., accum2.into());

            let result = match instr.bits(21..23) {
                // UMULL{cond}{S} RdLo,RdHi,Rm,Rs
                0 => self.execute_umlal(update_cond, value1, value2, 0),
                // UMLAL{cond}{S} RdLo,RdHi,Rm,Rs
                1 => self.execute_umlal(update_cond, value1, value2, accum_dword),
                // SMULL{cond}{S} RdLo,RdHi,Rm,Rs
                2 => self.execute_smlal(update_cond, value1 as i32, value2 as i32, 0),
                // SMLAL{cond}{S} RdLo,RdHi,Rm,Rs
                3 => self.execute_smlal(
                    update_cond,
                    value1 as i32,
                    value2 as i32,
                    accum_dword as i64,
                ),
                _ => unreachable!(),
            };

            self.reg.r[r_accum_or_lo] = result as u32;
            self.reg.r[r_dst_or_hi] = result.bits(32..) as u32;
        } else {
            // 32-bit result written to Rd.
            self.reg.r[r_dst_or_hi] = if instr.bit(21) {
                // MLA{cond}{S} Rd,Rm,Rs,Rn
                self.execute_mla(update_cond, value1, value2, accum1) as u32
            } else {
                // MUL{cond}{S} Rd,Rm,Rs
                self.execute_mla(update_cond, value1, value2, 0) as u32
            };
        }
    }

    /// PSR transfer.
    fn execute_arm_psr_transfer(&mut self, instr: u32) {
        // TODO: 1S
        let use_spsr = instr.bit(22);

        if instr.bit(21) {
            let value = if instr.bit(25) {
                // Immediate operand.
                #[allow(clippy::cast_possible_truncation)]
                self.execute_ror(false, false, instr.bits(..8), 2 * (instr.bits(8..12) as u8))
            } else {
                // Register operand.
                self.reg.r[r_index(instr, 0)]
            };

            // MSR{cond} Psr{_field},Op
            self.execute_msr(use_spsr, instr.bit(19), instr.bit(16), value);
        } else {
            // MRS{cond} Rd,Psr
            self.reg.r[r_index(instr, 12)] = self.execute_mrs(use_spsr);
        }
    }

    /// Single data transfer.
    fn execute_arm_single_transfer(&mut self, bus: &mut impl Bus, instr: u32) {
        // TODO: For normal LDR: 1S+1N+1I. For LDR PC: 2S+2N+1I. For STR: 2N.
        let preindex = instr.bit(24);
        let transfer_byte = instr.bit(22);
        let force_user = !preindex && instr.bit(21);
        let load = instr.bit(20);

        let r_base_addr = r_index(instr, 16);
        let r_src_or_dst = r_index(instr, 12);

        #[allow(clippy::cast_possible_truncation)]
        let offset = if instr.bit(25) {
            // Register offset shifted by immediate.
            let shift_offset = instr.bits(7..12) as u8;
            let value = self.reg.r[r_index(instr, 0)];

            self.execute_shift_operand(instr.bits(5..7) as _, false, true, value, shift_offset)
        } else {
            // Immediate offset.
            instr.bits(..12)
        };

        let base_addr = self.reg.r[r_base_addr];
        let final_addr = if instr.bit(23) {
            base_addr.wrapping_add(offset)
        } else {
            base_addr.wrapping_sub(offset)
        };
        let transfer_addr = if preindex { final_addr } else { base_addr };

        let saved_mode = self.reg.cpsr.mode;
        if force_user {
            self.reg.change_mode(OperationMode::User);
        }

        if load {
            // LDR{cond}{B}{T} Rd,<Address>
            self.reg.r[r_src_or_dst] = if transfer_byte {
                Self::execute_ldrb_or_ldsb(bus, transfer_addr, false)
            } else {
                Self::execute_ldr(bus, transfer_addr)
            };

            if r_src_or_dst == PC_INDEX {
                self.reload_pipeline(bus);
            }
        } else {
            let mut value = self.reg.r[r_src_or_dst];
            if r_src_or_dst == PC_INDEX {
                // PC reads as an extra instruction ahead here. (PC+12)
                value = value.wrapping_add(self.reg.cpsr.state.instr_size());
            }

            // STR{cond}{B}{T} Rd,<Address>
            if transfer_byte {
                #[allow(clippy::cast_possible_truncation)]
                Self::execute_strb(bus, transfer_addr, value as u8);
            } else {
                Self::execute_str(bus, transfer_addr, value);
            }
        }

        if force_user {
            self.reg.change_mode(saved_mode);
        }

        if (instr.bit(21) || !preindex) && !(load && r_base_addr == r_src_or_dst) {
            // Write-back to Rn.
            self.reg.r[r_base_addr] = final_addr;
            if r_base_addr == PC_INDEX {
                self.reload_pipeline(bus);
            }
        }
    }

    /// Half-word and signed data transfer.
    fn execute_arm_hword_and_signed_transfer(&mut self, bus: &mut impl Bus, instr: u32) {
        // TODO: For Normal LDR, 1S+1N+1I. For LDR PC, 2S+2N+1I. For STRH 2N.
        let preindex = instr.bit(24);
        let load = instr.bit(20);

        let r_base_addr = r_index(instr, 16);
        let r_src_or_dst = r_index(instr, 12);

        let offset = if instr.bit(22) {
            // Immediate offset.
            instr.bits(..4).with_bits(4.., instr.bits(8..12))
        } else {
            // Register offset.
            self.reg.r[r_index(instr, 0)]
        };

        let base_addr = self.reg.r[r_base_addr];
        let final_addr = if instr.bit(23) {
            base_addr.wrapping_add(offset)
        } else {
            base_addr.wrapping_sub(offset)
        };
        let transfer_addr = if preindex { final_addr } else { base_addr };

        let op = instr.bits(5..7);
        if load {
            self.reg.r[r_src_or_dst] = match op {
                // Reserved. TODO: how does it behave?
                0 => self.reg.r[r_src_or_dst],
                // LDR{cond}H Rd,<Address>
                1 => Self::execute_ldrh_or_ldsh(bus, transfer_addr, false),
                // LDR{cond}SB Rd,<Address>
                2 => Self::execute_ldrb_or_ldsb(bus, transfer_addr, true),
                // LDR{cond}SH Rd,<Address>
                3 => Self::execute_ldrh_or_ldsh(bus, transfer_addr, true),
                _ => unreachable!(),
            };

            if r_src_or_dst == PC_INDEX {
                self.reload_pipeline(bus);
            }
        } else {
            let mut value = self.reg.r[r_src_or_dst];
            if r_src_or_dst == PC_INDEX {
                // PC reads as an extra instruction ahead here. (PC+12)
                value = value.wrapping_add(self.reg.cpsr.state.instr_size());
            }

            if op == 1 {
                // STR{cond}H Rd,<Address>; other opcodes are reserved.
                #[allow(clippy::cast_possible_truncation)]
                Self::execute_strh(bus, transfer_addr, value as u16);
            }
        }

        if (instr.bit(21) || !preindex) && !(load && r_base_addr == r_src_or_dst) {
            // Write-back to Rn.
            self.reg.r[r_base_addr] = final_addr;
            if r_base_addr == PC_INDEX {
                self.reload_pipeline(bus);
            }
        }
    }
}

#[allow(
    clippy::unusual_byte_groupings,
    clippy::too_many_lines,
    clippy::unnecessary_cast // lint doesn't work properly with negative literals
)]
#[cfg(test)]
mod tests {
    use crate::{
        arm7tdmi::{
            isa::tests::InstrTest,
            reg::{OperationMode, LR_INDEX},
        },
        bus::{tests::VecBus, BusExt},
    };

    use super::*;

    use intbits::Bits;

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
    fn execute_arm_data_processing_decode() {
        // AND{cond}{S} Rd,Rn,Op2; mostly test argument decoding and handling here.
        // This also includes offset shifting opcodes.

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

        // AL S R14,R0,#11100001b,ROR #6
        InstrTest::new_arm(0b1110_00_1_0000_1_0000_1110_0011_11100001)
            .setup(&|cpu| cpu.reg.r[0] = u32::MAX)
            .assert_r(0, u32::MAX)
            .assert_r(14, 0b11.with_bits(26.., 0b10_0001))
            .assert_signed()
            .assert_carry()
            .run();

        // AL R14,R0,#11100001b,ROR #6
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

        // AL S R9,R0,R11,LSL #30
        InstrTest::new_arm(0b1110_00_0_0000_1_0000_1001_11110_00_0_1011)
            .setup(&|cpu| {
                cpu.reg.r[0] = 0b111 << 29;
                cpu.reg.r[11] = 0b111;
            })
            .assert_r(0, 0b111 << 29)
            .assert_r(9, 0b11 << 30)
            .assert_r(11, 0b111)
            .assert_carry()
            .assert_signed()
            .run();

        // AL S R9,R0,R11,LSL #0
        InstrTest::new_arm(0b1110_00_0_0000_1_0000_1001_00000_00_0_1011)
            .setup(&|cpu| {
                cpu.reg.r[0] = 0b111 << 29;
                cpu.reg.r[11] = 0b111;
            })
            .assert_r(0, 0b111 << 29)
            .assert_r(11, 0b111)
            .assert_zero()
            .run();

        // AL S R9,R0,R11,LSR #2
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

        // AL S R9,R0,R11,LSR #0
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

        // AL S R9,R0,R11,LSR #0
        InstrTest::new_arm(0b1110_00_0_0000_1_0000_1001_00000_01_0_1011)
            .setup(&|cpu| {
                cpu.reg.r[0] = 0b111 << 29;
                cpu.reg.r[11] = u32::MAX.with_bit(31, false);
            })
            .assert_r(0, 0b111 << 29)
            .assert_r(11, u32::MAX.with_bit(31, false))
            .assert_zero()
            .run();

        // AL S R9,R0,R11,ASR #2
        InstrTest::new_arm(0b1110_00_0_0000_1_0000_1001_00010_10_0_1011)
            .setup(&|cpu| {
                cpu.reg.r[0] = 0b111 << 29;
                cpu.reg.r[11] = u32::MAX;
            })
            .assert_r(0, 0b111 << 29)
            .assert_r(9, 0b111 << 29)
            .assert_r(11, u32::MAX)
            .assert_carry()
            .assert_signed()
            .run();

        // AL S R9,R0,R11,ASR #0
        InstrTest::new_arm(0b1110_00_0_0000_1_0000_1001_00000_10_0_1011)
            .setup(&|cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[11] = 1 << 31;
            })
            .assert_r(0, u32::MAX)
            .assert_r(9, u32::MAX)
            .assert_r(11, 1 << 31)
            .assert_carry()
            .assert_signed()
            .run();

        // AL S R9,R0,R11,ROR #3
        InstrTest::new_arm(0b1110_00_0_0000_1_0000_1001_00011_11_0_1011)
            .setup(&|cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[11] = 0b101_0101;
            })
            .assert_r(0, u32::MAX)
            .assert_r(9, 0b1010.with_bits(29.., 0b101))
            .assert_r(11, 0b101_0101)
            .assert_carry()
            .assert_signed()
            .run();

        // AL S R9,R0,R11,ROR #0
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

        // AL S R9,R0,R11,ROR #0
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
            .assert_signed()
            .run();

        // AL S R9,R0,R15,LSL #1
        InstrTest::new_arm(0b1110_00_0_0000_1_0000_1001_00001_00_0_1111)
            .setup(&|cpu| cpu.reg.r[0] = u32::MAX)
            .assert_r(0, u32::MAX)
            .assert_r(9, 8 << 1)
            .run();

        // AL S R9,R15,R0,LSL #0
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
            .assert_signed()
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
    #[allow(clippy::cast_sign_loss)]
    fn execute_arm_data_processing_ops() {
        // AND plus argument handling is already tested in execute_arm_data_processing_decode.
        // We'll test the other ops here with and without the condition bit set
        // (except TST, TEQ, CMP and CMN).

        // EOR{cond}{S} Rd,Rn,Op2
        // AL S R14,R0,#10101010b
        InstrTest::new_arm(0b1110_00_1_0001_1_0000_1110_0000_10101010)
            .setup(&|cpu| cpu.reg.r[0] = 0b1100_0011.with_bit(31, true))
            .assert_r(0, 0b1100_0011.with_bit(31, true))
            .assert_r(14, 0b110_1001.with_bit(31, true))
            .assert_signed()
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
            .assert_signed()
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
            .assert_signed()
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
            .assert_signed()
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
            .assert_signed()
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

        // TST{cond} Rn,Op2
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

        // TEQ{cond} Rn,Op2
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

        // CMP{cond} Rn,Op2
        // AL S R0,#20
        InstrTest::new_arm(0b1110_00_1_1010_1_0000_1111_0000_00010100)
            .setup(&|cpu| cpu.reg.r[0] = 15)
            .assert_r(0, 15)
            .assert_signed()
            .run();

        // AL R0,#20
        InstrTest::new_arm(0b1110_00_1_1010_1_0000_0000_0000_00010100)
            .setup(&|cpu| cpu.reg.r[0] = 20)
            .assert_r(0, 20)
            .assert_zero()
            .assert_carry()
            .run();

        // CMN{cond} Rn,Op2
        // AL R0,#3
        InstrTest::new_arm(0b1110_00_1_1011_1_0000_0000_0000_00000011)
            .setup(&|cpu| cpu.reg.r[0] = -15 as _)
            .assert_r(0, -15 as _)
            .assert_signed()
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
            .assert_signed()
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
            .assert_signed()
            .run();

        // AL R14,#0
        InstrTest::new_arm(0b1110_00_1_1111_0_0000_1110_0000_00000001)
            .assert_r(14, u32::MAX.with_bit(0, false))
            .run();
    }

    #[test]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn execute_arm_multiply() {
        // MUL{cond}{S} Rd,Rm,Rs
        // AL S R14,R2,R0
        InstrTest::new_arm(0b1110_000_0000_1_1110_0000_0000_1001_0010)
            .setup(&|cpu| {
                cpu.reg.r[0] = 200_123;
                cpu.reg.r[2] = 12_024;
            })
            .assert_r(0, 200_123)
            .assert_r(2, 12_024)
            .assert_r(14, 200_123 * 12_024)
            .assert_signed()
            .run();

        // AL R14,R2,R0
        InstrTest::new_arm(0b1110_000_0000_0_1110_0000_0000_1001_0010)
            .setup(&|cpu| {
                cpu.reg.r[0] = 200_123;
                cpu.reg.r[2] = 12_024;
            })
            .assert_r(0, 200_123)
            .assert_r(2, 12_024)
            .assert_r(14, 200_123 * 12_024)
            .run();

        // MLA{cond}{S} Rd,Rm,Rs,Rn
        // AL S R14,R2,R0,R3
        InstrTest::new_arm(0b1110_000_0001_1_1110_0011_0000_1001_0010)
            .setup(&|cpu| {
                cpu.reg.r[0] = 200_123;
                cpu.reg.r[2] = 12_024;
                cpu.reg.r[3] = 1337;
            })
            .assert_r(0, 200_123)
            .assert_r(2, 12_024)
            .assert_r(3, 1337)
            .assert_r(14, 200_123 * 12_024 + 1337)
            .assert_signed()
            .run();

        // AL R14,R2,R0
        InstrTest::new_arm(0b1110_000_0001_0_1110_0011_0000_1001_0010)
            .setup(&|cpu| {
                cpu.reg.r[0] = 200_123;
                cpu.reg.r[2] = 12_024;
                cpu.reg.r[3] = 1337;
            })
            .assert_r(0, 200_123)
            .assert_r(2, 12_024)
            .assert_r(3, 1337)
            .assert_r(14, 200_123 * 12_024 + 1337)
            .run();

        // UMULL{cond}{S} RdLo,RdHi,Rs,Rn
        // AL S R2,R14,R0,R3
        #[allow(clippy::cast_lossless)]
        InstrTest::new_arm(0b1110_000_0100_1_1110_0010_0000_1001_0011)
            .setup(&|cpu| {
                cpu.reg.r[0] = 30;
                cpu.reg.r[3] = -2 as _;
            })
            .assert_r(0, 30)
            .assert_r(3, -2 as _)
            .assert_r(2, (30u64 * (-2i32 as u32 as u64)) as u32)
            .assert_r(14, (30u64 * (-2i32 as u32 as u64)).bits(32..) as _)
            .run();

        // AL S R2,R14,R0,R3
        InstrTest::new_arm(0b1110_000_0100_1_1110_0010_0000_1001_0011)
            .setup(&|cpu| {
                cpu.reg.r[0] = 200_123;
                cpu.reg.r[3] = 10_712;
            })
            .assert_r(0, 200_123)
            .assert_r(3, 10_712)
            .assert_r(2, (200_123 * 10_712) as u32)
            .assert_r(14, (200_123 * 10_712).bits(32..))
            .run();

        // AL S R2,R14,R0,R3
        InstrTest::new_arm(0b1110_000_0100_1_1110_0010_0000_1001_0011)
            .setup(&|cpu| cpu.reg.r[0] = 200_123)
            .assert_r(0, 200_123)
            .assert_zero()
            .run();

        // AL R2,R14,R0,R3
        InstrTest::new_arm(0b1110_000_0100_0_1110_0010_0000_1001_0011)
            .setup(&|cpu| cpu.reg.r[0] = 200_123)
            .assert_r(0, 200_123)
            .run();

        // UMLAL{cond}{S} RdLo,RdHi,Rs,Rn
        // AL S R2,R14,R0,R3
        InstrTest::new_arm(0b1110_000_0101_1_1110_0010_0000_1001_0011)
            .setup(&|cpu| {
                cpu.reg.r[0] = 3;
                cpu.reg.r[3] = 2;
                cpu.reg.r[2] = u32::MAX - 6;
                cpu.reg.r[14] = u32::MAX;
            })
            .assert_r(0, 3)
            .assert_r(3, 2)
            .assert_r(2, u32::MAX)
            .assert_r(14, u32::MAX)
            .assert_signed()
            .run();

        // AL S R2,R14,R0,R3
        InstrTest::new_arm(0b1110_000_0101_1_1110_0010_0000_1001_0011)
            .setup(&|cpu| {
                cpu.reg.r[0] = 3;
                cpu.reg.r[3] = 2;
                cpu.reg.r[2] = 13;
                cpu.reg.r[14] = u32::MAX.with_bit(31, false);
            })
            .assert_r(0, 3)
            .assert_r(3, 2)
            .assert_r(2, 19)
            .assert_r(14, u32::MAX.with_bit(31, false))
            .run();

        // AL R2,R14,R0,R3
        InstrTest::new_arm(0b1110_000_0101_1_1110_0010_0000_1001_0011)
            .setup(&|cpu| {
                cpu.reg.r[0] = 3;
                cpu.reg.r[3] = 2;
                cpu.reg.r[2] = u32::MAX - 6;
                cpu.reg.r[14] = u32::MAX;
            })
            .assert_r(0, 3)
            .assert_r(3, 2)
            .assert_r(2, u32::MAX)
            .assert_r(14, u32::MAX)
            .assert_signed()
            .run();

        // SMULL{cond}{S} RdLo,RdHi,Rs,Rn
        // AL S R2,R14,R0,R3
        InstrTest::new_arm(0b1110_000_0110_1_1110_0010_0000_1001_0011)
            .setup(&|cpu| {
                cpu.reg.r[0] = 30;
                cpu.reg.r[3] = -2 as _;
            })
            .assert_r(0, 30)
            .assert_r(3, -2 as _)
            .assert_r(2, -60 as _)
            .assert_r(14, u32::MAX)
            .assert_signed()
            .run();

        // AL R2,R14,R0,R3
        InstrTest::new_arm(0b1110_000_0110_0_1110_0010_0000_1001_0011)
            .setup(&|cpu| {
                cpu.reg.r[0] = 30;
                cpu.reg.r[3] = -2 as _;
            })
            .assert_r(0, 30)
            .assert_r(3, -2 as _)
            .assert_r(2, -60 as _)
            .assert_r(14, u32::MAX)
            .run();

        // SMLAL{cond}{S} RdLo,RdHi,Rs,Rn
        // AL S R2,R14,R0,R3
        InstrTest::new_arm(0b1110_000_0111_1_1110_0010_0000_1001_0011)
            .setup(&|cpu| {
                cpu.reg.r[0] = -30 as _;
                cpu.reg.r[3] = -2 as _;
                cpu.reg.r[2] = -71 as _;
                cpu.reg.r[14] = u32::MAX;
            })
            .assert_r(0, -30 as _)
            .assert_r(3, -2 as _)
            .assert_r(2, -11 as _)
            .assert_r(14, u32::MAX)
            .assert_signed()
            .run();

        // AL R2,R14,R0,R3
        InstrTest::new_arm(0b1110_000_0111_0_1110_0010_0000_1001_0011)
            .setup(&|cpu| {
                cpu.reg.r[0] = -30 as _;
                cpu.reg.r[3] = -2 as _;
                cpu.reg.r[2] = -71 as _;
                cpu.reg.r[14] = u32::MAX;
            })
            .assert_r(0, -30 as _)
            .assert_r(3, -2 as _)
            .assert_r(2, -11 as _)
            .assert_r(14, u32::MAX)
            .run();
    }

    #[test]
    fn execute_arm_psr_transfer() {
        // MRS{cond} Rd,Psr
        // AL R11,CPSR
        InstrTest::new_arm(0b1110_00_0_10_0_0_0_1111_1011_000000000000)
            .assert_r(11, 0b11_0_10011) // Supervisor (SVC) mode, ARM state, IRQ & FIQ disabled
            .run();

        // AL R14,CPSR
        InstrTest::new_arm(0b1110_00_0_10_0_0_0_1111_1110_000000000000)
            .setup(&|cpu| {
                cpu.reg.change_mode(OperationMode::System);
                cpu.reg.cpsr.fiq_disabled = false;
                cpu.reg.cpsr.signed = true;
                cpu.reg.cpsr.carry = true;
            })
            .assert_r(14, 0b10_0_11111.with_bits(28.., 0b1010))
            .assert_fiq_enabled()
            .assert_signed()
            .assert_carry()
            .run();

        // AL R7,SPSR_svc
        InstrTest::new_arm(0b1110_00_0_10_1_0_0_1111_0111_000000000000)
            .setup(&|cpu| {
                cpu.reg.spsr.mode = OperationMode::System;
                cpu.reg.spsr.irq_disabled = true;
                cpu.reg.spsr.fiq_disabled = false;
                cpu.reg.spsr.signed = true;
                cpu.reg.spsr.carry = true;
            })
            .assert_r(7, 0b10_0_11111.with_bits(28.., 0b1010))
            .run();

        // MSR{cond} Psr{_field},Op
        // AL CPSR_f,#0101b,ROR #4
        let cpu = InstrTest::new_arm(0b1110_00_1_10_0_1_0_1000_1111_0010_00000101)
            .assert_zero()
            .assert_overflow()
            .run();

        assert_eq!(cpu.reg.cpsr.mode, OperationMode::Supervisor);

        // AL SPSR_svc_f,#0101b,ROR #4
        let cpu = InstrTest::new_arm(0b1110_00_1_10_1_1_0_1000_1111_0010_00000101).run();

        assert!(cpu.reg.spsr.zero);
        assert!(cpu.reg.spsr.overflow);
        assert!(!cpu.reg.spsr.signed);
        assert!(!cpu.reg.spsr.carry);
        assert!(!cpu.reg.spsr.irq_disabled);
        assert!(!cpu.reg.spsr.fiq_disabled);

        // AL CPSR_c,#01110000b
        let cpu = InstrTest::new_arm(0b1110_00_1_10_0_1_0_0001_1111_0000_01110000)
            .assert_irq_enabled()
            .run();

        assert_eq!(cpu.reg.cpsr.mode, OperationMode::User);

        // AL SPSR_svc_fc,#11110000b
        let cpu = InstrTest::new_arm(0b1110_00_1_10_1_1_0_1001_1111_0000_11110000).run();

        assert!(!cpu.reg.spsr.zero);
        assert!(!cpu.reg.spsr.overflow);
        assert!(!cpu.reg.spsr.signed);
        assert!(!cpu.reg.spsr.carry);
        assert!(cpu.reg.spsr.irq_disabled);
        assert!(cpu.reg.spsr.fiq_disabled);
        assert_eq!(cpu.reg.spsr.mode, OperationMode::User);

        // AL CPSR_f,R10
        let cpu = InstrTest::new_arm(0b1110_00_0_10_0_1_0_1000_1111_00000000_1010)
            .setup(&|cpu| cpu.reg.r[10] = 0b00_1_11111.with_bits(28.., 0b1010))
            .assert_r(10, 0b00_1_11111.with_bits(28.., 0b1010))
            .assert_signed()
            .assert_carry()
            .run();

        assert_eq!(cpu.reg.cpsr.mode, OperationMode::Supervisor);

        // AL CPSR_c,R10
        let cpu = InstrTest::new_arm(0b1110_00_0_10_0_1_0_0001_1111_00000000_1010)
            .setup(&|cpu| cpu.reg.r[10] = 0b00_1_11111.with_bits(28.., 0b1010))
            .assert_r(10, 0b00_1_11111.with_bits(28.., 0b1010))
            .assert_irq_enabled()
            .assert_fiq_enabled()
            .run();

        assert_eq!(cpu.reg.cpsr.mode, OperationMode::System);

        // AL CPSR_fc,R10
        let cpu = InstrTest::new_arm(0b1110_00_0_10_0_1_0_1001_1111_00000000_1010)
            .setup(&|cpu| cpu.reg.r[10] = 0b00_1_11111.with_bits(28.., 0b1010))
            .assert_r(10, 0b00_1_11111.with_bits(28.., 0b1010))
            .assert_signed()
            .assert_carry()
            .assert_irq_enabled()
            .assert_fiq_enabled()
            .run();

        assert_eq!(cpu.reg.cpsr.mode, OperationMode::System);

        // AL CPSR_fc,R10
        let cpu = InstrTest::new_arm(0b1110_00_0_10_0_1_0_1001_1111_00000000_1010)
            .setup(&|cpu| {
                cpu.reg.change_mode(OperationMode::User);
                cpu.reg.r[10] = 0b00_1_11111.with_bits(28.., 0b1010);
            })
            .assert_r(10, 0b00_1_11111.with_bits(28.., 0b1010))
            .assert_signed()
            .assert_carry()
            .run();

        assert_eq!(cpu.reg.cpsr.mode, OperationMode::User);
    }

    #[test]
    fn execute_arm_single_transfer() {
        let mut bus = VecBus::new(100);
        bus.write_word(4, 0xfefe_dede);
        bus.write_word(12, 0xbeef_feeb);
        bus.write_word(20, 0xabcd_ef98);

        // LDR{cond}{B}{T} Rd,<Address>
        // AL R12,[R1],<#+8>
        InstrTest::new_arm(0b1110_01_0010_0_1_0001_1100_000000001000)
            .setup(&|cpu| cpu.reg.r[1] = 12)
            .assert_r(1, 20)
            .assert_r(12, 0xbeef_feeb)
            .run_with_bus(&mut bus);

        // AL R12,[R1],<#-8>
        InstrTest::new_arm(0b1110_01_0000_0_1_0001_1100_000000001000)
            .setup(&|cpu| cpu.reg.r[1] = 12)
            .assert_r(1, 4)
            .assert_r(12, 0xbeef_feeb)
            .run_with_bus(&mut bus);

        // AL R12,[R1],+R7,LSR #2
        InstrTest::new_arm(0b1110_01_1010_0_1_0001_1100_00010_01_0_0111)
            .setup(&|cpu| {
                cpu.reg.r[1] = 12;
                cpu.reg.r[7] = 8 << 2;
            })
            .assert_r(1, 20)
            .assert_r(7, 8 << 2)
            .assert_r(12, 0xbeef_feeb)
            .run_with_bus(&mut bus);

        // AL R12,[R1],-R7,LSR #2
        InstrTest::new_arm(0b1110_01_1000_0_1_0001_1100_00010_01_0_0111)
            .setup(&|cpu| {
                cpu.reg.r[1] = 12;
                cpu.reg.r[7] = 8 << 2;
            })
            .assert_r(1, 4)
            .assert_r(7, 8 << 2)
            .assert_r(12, 0xbeef_feeb)
            .run_with_bus(&mut bus);

        // AL T R12,[R1],-R7,LSR #2
        InstrTest::new_arm(0b1110_01_1000_1_1_0001_1100_00010_01_0_0111)
            .setup(&|cpu| {
                cpu.reg.r[1] = 12;
                cpu.reg.r[7] = 8 << 2;
            })
            .assert_r(1, 4)
            .assert_r(7, 8 << 2)
            .assert_r(12, 0xbeef_feeb)
            .run_with_bus(&mut bus);

        // AL B R12,[R1],-R7,LSR #2
        InstrTest::new_arm(0b1110_01_1001_0_1_0001_1100_00010_01_0_0111)
            .setup(&|cpu| {
                cpu.reg.r[1] = 12;
                cpu.reg.r[7] = 8 << 2;
            })
            .assert_r(1, 4)
            .assert_r(7, 8 << 2)
            .assert_r(12, 0xeb)
            .run_with_bus(&mut bus);

        // AL R12,[R1,-R7,LSR #2]
        InstrTest::new_arm(0b1110_01_1100_0_1_0001_1100_00010_01_0_0111)
            .setup(&|cpu| {
                cpu.reg.r[1] = 12;
                cpu.reg.r[7] = 8 << 2;
            })
            .assert_r(1, 12)
            .assert_r(7, 8 << 2)
            .assert_r(12, 0xfefe_dede)
            .run_with_bus(&mut bus);

        // AL R12,[R1,-R7,LSR #2]!
        InstrTest::new_arm(0b1110_01_1100_1_1_0001_1100_00010_01_0_0111)
            .setup(&|cpu| {
                cpu.reg.r[1] = 12;
                cpu.reg.r[7] = 8 << 2;
            })
            .assert_r(1, 4)
            .assert_r(7, 8 << 2)
            .assert_r(12, 0xfefe_dede)
            .run_with_bus(&mut bus);

        // AL R12,[R15,+R7,LSR #2]!
        InstrTest::new_arm(0b1110_01_1110_1_1_1111_1100_00010_01_0_0111)
            .setup(&|cpu| {
                cpu.reg.r[PC_INDEX] = 12;
                cpu.reg.r[7] = 8 << 2;
            })
            .assert_r(PC_INDEX, 20 + 8)
            .assert_r(7, 8 << 2)
            .assert_r(12, 0xabcd_ef98)
            .run_with_bus(&mut bus);

        // AL R15,[R1,+R7,LSR #2]!
        bus.assert_oob(&|bus| {
            InstrTest::new_arm(0b1110_01_1110_1_1_0001_1111_00010_01_0_0111)
                .setup(&|cpu| {
                    cpu.reg.r[1] = 12;
                    cpu.reg.r[7] = 8 << 2;
                })
                .assert_r(1, 20)
                .assert_r(7, 8 << 2)
                .assert_r(PC_INDEX, 0xabcd_ef98 + 8)
                .run_with_bus(bus);
        });

        // AL R15,[R15,+R7,LSR #2]!
        bus.assert_oob(&|bus| {
            InstrTest::new_arm(0b1110_01_1110_1_1_1111_1111_00010_01_0_0111)
                .setup(&|cpu| {
                    cpu.reg.r[PC_INDEX] = 12;
                    cpu.reg.r[7] = 8 << 2;
                })
                .assert_r(PC_INDEX, 0xabcd_ef98 + 8)
                .assert_r(7, 8 << 2)
                .run_with_bus(bus);
        });

        // STR{cond}{B}{T} Rd,<Address>
        // Decoding is already mostly tested for above, and the shared code for STR instructions
        // are already tested for in Thumb tests.

        // AL R15,[R1,<#+24>]!
        InstrTest::new_arm(0b1110_01_0110_1_0_0001_1111_000000011000)
            .setup(&|cpu| {
                cpu.reg.r[1] = 8;
                cpu.reg.r[PC_INDEX] = 0x1337_7331;
            })
            .assert_r(1, 32)
            .assert_r(PC_INDEX, 0x1337_7331)
            .run_with_bus(&mut bus);

        assert_eq!(bus.read_word(32), 0x1337_7331 + 4);

        // AL B R15,[R1,<#+24>]!
        InstrTest::new_arm(0b1110_01_0111_1_0_0001_1111_000000011000)
            .setup(&|cpu| {
                cpu.reg.r[1] = 16;
                cpu.reg.r[PC_INDEX] = 0x1337_7331;
            })
            .assert_r(1, 40)
            .assert_r(PC_INDEX, 0x1337_7331)
            .run_with_bus(&mut bus);

        assert_eq!(bus.read_word(40), 0x31 + 4);
    }

    #[test]
    fn execute_arm_hword_and_signed_transfer() {
        let mut bus = VecBus::new(44);
        bus.write_word(0, 0xceec_0a0c);
        bus.write_word(4, 0xfefe_dede);
        bus.write_word(12, 0xbeef_feeb);
        bus.write_word(20, 0xabcd_ef98);

        // LDR{cond}H Rd,<Address>
        // AL H R5,[R8],-R1
        InstrTest::new_arm(0b1110_000_000_0_1_1000_0101_0000_1_01_1_0001)
            .setup(&|cpu| {
                cpu.reg.r[8] = 22;
                cpu.reg.r[1] = 8;
            })
            .assert_r(8, 14)
            .assert_r(1, 8)
            .assert_r(5, 0xabcd)
            .run_with_bus(&mut bus);

        // AL H R5,[R8],<#+18>
        InstrTest::new_arm(0b1110_000_011_0_1_1000_0101_0001_1_01_1_0010)
            .setup(&|cpu| cpu.reg.r[8] = 2)
            .assert_r(8, 20)
            .assert_r(5, 0xceec)
            .run_with_bus(&mut bus);

        // LDR{cond}SB Rd,<Address>
        // AL SB R5,[R8,-R1]
        InstrTest::new_arm(0b1110_000_100_0_1_1000_0101_0000_1_10_1_0001)
            .setup(&|cpu| {
                cpu.reg.r[8] = 22;
                cpu.reg.r[1] = 19;
            })
            .assert_r(8, 22)
            .assert_r(1, 19)
            .assert_r(5, 0xffff_ffce)
            .run_with_bus(&mut bus);

        // AL SB R5,[R8,-R1]!
        InstrTest::new_arm(0b1110_000_100_1_1_1000_0101_0000_1_10_1_0001)
            .setup(&|cpu| {
                cpu.reg.r[8] = 22;
                cpu.reg.r[1] = 17;
            })
            .assert_r(8, 5)
            .assert_r(1, 17)
            .assert_r(5, 0xffff_ffde)
            .run_with_bus(&mut bus);

        // LDR{cond}SH Rd,<Address>
        // AL SH R5,[R15,<#+7>]!
        #[allow(clippy::cast_sign_loss)]
        InstrTest::new_arm(0b1110_000_111_1_1_1111_0101_0000_1_11_1_0111)
            .setup(&|cpu| cpu.reg.r[PC_INDEX] = -7i32 as u32)
            .assert_r(5, 0x0a0c)
            .run_with_bus(&mut bus);

        // AL SH R15,[R3,<#+6>]
        bus.assert_oob(&|bus| {
            InstrTest::new_arm(0b1110_000_111_0_1_0011_1111_0000_1_11_1_0110)
                .setup(&|cpu| cpu.reg.r[3] = 8)
                .assert_r(3, 8)
                .assert_r(PC_INDEX, (0xffff_beef & !0b11) + 8)
                .run_with_bus(bus);
        });

        // STR{cond}H Rd,<Address>
        // AL H R15,[R3,<#+12>]!
        InstrTest::new_arm(0b1110_000_111_1_0_0011_1111_0000_1_01_1_1100)
            .setup(&|cpu| {
                cpu.reg.r[3] = 8;
                cpu.reg.r[PC_INDEX] = 0x20;
            })
            .assert_r(3, 20)
            .assert_r(PC_INDEX, 0x20)
            .run_with_bus(&mut bus);

        assert_eq!(bus.read_word(20), 0xabcd_0020 + 4);

        // AL H R15,[R15,<#+6>]!
        InstrTest::new_arm(0b1110_000_111_1_0_1111_1111_0000_1_01_1_0110)
            .setup(&|cpu| cpu.reg.r[PC_INDEX] = 0x20)
            .assert_r(PC_INDEX, (0x26 & !0b11) + 8)
            .run_with_bus(&mut bus);

        assert_eq!(bus.read_word(0x20 + 6), 0x000_0020 + 4);
    }
}
