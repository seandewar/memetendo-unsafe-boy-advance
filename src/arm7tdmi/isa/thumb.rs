use bitmatch::bitmatch;
use intbits::Bits;

use crate::{
    arbitrary_sign_extend,
    arm7tdmi::{
        reg::{OperationState, LR_INDEX, PC_INDEX, SP_INDEX},
        Cpu, Exception,
    },
    bus::Bus,
};

use super::BlockTransferFlags;

fn r_index(instr: u16, pos: u8) -> usize {
    instr.bits(pos..(pos + 3)).into()
}

impl Cpu {
    #[bitmatch]
    pub(in crate::arm7tdmi) fn execute_thumb(&mut self, bus: &mut impl Bus, instr: u16) {
        debug_assert!(self.reg.cpsr.state == OperationState::Thumb);

        // TODO: SWI is 2S+1N
        #[allow(clippy::cast_possible_truncation)]
        #[bitmatch]
        match instr.bits(8..) as u8 {
            "1011_0000" => self.execute_thumb13(instr),
            "1101_1111" => {
                self.enter_exception(bus, Exception::SoftwareInterrupt);
            }
            "0100_00??" => self.execute_thumb4(instr),
            "0100_01??" => self.execute_thumb5(bus, instr),
            "0001_1???" => self.execute_thumb2(instr),
            "0100_1???" => self.execute_thumb6(bus, instr),
            "1110_0???" => self.execute_thumb18(bus, instr),
            "0101_????" => self.execute_thumb7_or_thumb8(bus, instr),
            "1000_????" => self.execute_thumb10(bus, instr),
            "1001_????" => self.execute_thumb11(bus, instr),
            "1010_????" => self.execute_thumb12(instr),
            "1011_????" => self.execute_thumb14(bus, instr),
            "1100_????" => self.execute_thumb15(bus, instr),
            "1101_????" => self.execute_thumb16(bus, instr),
            "1111_????" => self.execute_thumb19(bus, instr),
            "000?_????" => self.execute_thumb1(instr),
            "001?_????" => self.execute_thumb3(instr),
            "011?_????" => self.execute_thumb9(bus, instr),
            _ => {}
        }
    }

    /// Thumb.1: Move shifted register.
    fn execute_thumb1(&mut self, instr: u16) {
        let value = self.reg.r[r_index(instr, 3)];
        #[allow(clippy::cast_possible_truncation)]
        let offset = instr.bits(6..11) as u8;

        self.reg.r[r_index(instr, 0)] = match instr.bits(11..13) {
            // LSL{S} Rd,Rs,#Offset
            0 => self.op_lsl(true, value, offset),
            // LSR{S} Rd,Rs,#Offset
            1 => self.op_lsr(true, true, value, offset),
            // ASR{S} Rd,Rs,#Offset
            2 => self.op_asr(true, true, value, offset),
            _ => unreachable!(),
        };
    }

    /// Thumb.2: Add or subtract.
    fn execute_thumb2(&mut self, instr: u16) {
        let value1 = self.reg.r[r_index(instr, 3)];
        let r = r_index(instr, 6);
        #[allow(clippy::cast_possible_truncation)]
        let value2 = r as u32;

        self.reg.r[r_index(instr, 0)] = match instr.bits(9..11) {
            // ADD{S} Rd,Rs,Rn
            0 => self.op_add(true, value1, self.reg.r[r]),
            // SUB{S} Rd,Rs,Rn
            1 => self.op_sub(true, value1, self.reg.r[r]),
            // ADD{S} Rd,Rs,#nn
            2 => self.op_add(true, value1, value2),
            // SUB{S} Rd,Rs,#nn
            3 => self.op_sub(true, value1, value2),
            _ => unreachable!(),
        };
    }

    /// Thumb.3: Move, compare, add or subtract immediate.
    fn execute_thumb3(&mut self, instr: u16) {
        let value = u32::from(instr.bits(..8));
        let r_dst = r_index(instr, 8);

        match instr.bits(11..13) {
            // MOV{S} Rd,#nn
            0 => self.reg.r[r_dst] = self.op_mov(true, value),
            // CMP{S} Rd,#nn
            1 => {
                self.op_sub(true, self.reg.r[r_dst], value);
            }
            // ADD{S} Rd,#nn
            2 => self.reg.r[r_dst] = self.op_add(true, self.reg.r[r_dst], value),
            // SUB{S} Rd,#nn
            3 => self.reg.r[r_dst] = self.op_sub(true, self.reg.r[r_dst], value),
            _ => unreachable!(),
        }
    }

    /// Thumb.4: ALU operations.
    #[allow(clippy::cast_possible_truncation)]
    fn execute_thumb4(&mut self, instr: u16) {
        let r_dst = r_index(instr, 0);
        let value = self.reg.r[r_index(instr, 3)];
        let offset = value as u8;

        match instr.bits(6..10) {
            // AND{S} Rd,Rs
            0 => self.reg.r[r_dst] = self.op_and(true, self.reg.r[r_dst], value),
            // EOR{S} Rd,Rs
            1 => self.reg.r[r_dst] = self.op_eor(true, self.reg.r[r_dst], value),
            // LSL{S} Rd,Rs
            2 => self.reg.r[r_dst] = self.op_lsl(true, self.reg.r[r_dst], offset),
            // LSR{S} Rd,Rs
            3 => self.reg.r[r_dst] = self.op_lsr(true, false, self.reg.r[r_dst], offset),
            // ASR{S} Rd,Rs
            4 => self.reg.r[r_dst] = self.op_asr(true, false, self.reg.r[r_dst], offset),
            // ADC{S} Rd,Rs
            5 => self.reg.r[r_dst] = self.op_adc(true, self.reg.r[r_dst], value),
            // SBC{S} Rd,Rs
            6 => self.reg.r[r_dst] = self.op_sbc(true, self.reg.r[r_dst], value),
            // ROR{S} Rd,Rs
            7 => self.reg.r[r_dst] = self.op_ror(true, false, self.reg.r[r_dst], offset),
            // TST Rd,Rs
            8 => {
                self.op_and(true, self.reg.r[r_dst], value);
            }
            // NEG{S} Rd,Rs
            9 => self.reg.r[r_dst] = self.op_sub(true, 0, value),
            // CMP Rd,Rs
            10 => {
                self.op_sub(true, self.reg.r[r_dst], value);
            }
            // CMN Rd,Rs
            11 => {
                self.op_add(true, self.reg.r[r_dst], value);
            }
            // ORR{S} Rd,Rs
            12 => self.reg.r[r_dst] = self.op_orr(true, self.reg.r[r_dst], value),
            // MUL{S} Rd,Rs
            13 => self.reg.r[r_dst] = self.op_mla(true, self.reg.r[r_dst], value, 0),
            // BIC{S} Rd,Rs
            14 => self.reg.r[r_dst] = self.op_bic(true, self.reg.r[r_dst], value),
            // MVN{S} Rd,Rs
            15 => self.reg.r[r_dst] = self.op_mvn(true, value),
            _ => unreachable!(),
        }
    }

    /// Thumb.5: Hi register operations or branch exchange.
    fn execute_thumb5(&mut self, bus: &impl Bus, instr: u16) {
        let r_src = r_index(instr, 3).with_bit(3, instr.bit(6));
        let value = self.reg.r[r_src];
        let op = instr.bits(8..10);

        if op == 3 {
            // BX Rs
            self.op_bx(bus, value);
            return;
        }

        let r_dst = r_index(instr, 0).with_bit(3, instr.bit(7));
        match op {
            // ADD Rd,Rs
            0 => self.reg.r[r_dst] = self.op_add(false, self.reg.r[r_dst], value),
            // CMP Rd,Rs
            1 => {
                self.op_sub(true, self.reg.r[r_dst], value);
            }
            // MOV Rd,Rs
            2 => self.reg.r[r_dst] = self.op_mov(false, value),
            _ => unreachable!(),
        }

        if op != 1 && r_dst == PC_INDEX {
            self.reload_pipeline(bus);
        }
    }

    /// Thumb.6: Load PC relative.
    fn execute_thumb6(&mut self, bus: &impl Bus, instr: u16) {
        let offset = u32::from(instr.bits(..8));
        let addr = self.reg.r[PC_INDEX].wrapping_add(offset * 4);

        // LDR Rd,[PC,#nn]
        self.reg.r[r_index(instr, 8)] = Self::op_ldr(bus, addr);
    }

    /// Thumb.7: Load or store with register offset, OR
    /// Thumb.8: Load or store sign-extended byte or half-word (if bit 9 is set in `instr`).
    #[allow(clippy::cast_possible_truncation)]
    fn execute_thumb7_or_thumb8(&mut self, bus: &mut impl Bus, instr: u16) {
        let r = r_index(instr, 0);
        let base_addr = self.reg.r[r_index(instr, 3)];
        let offset = self.reg.r[r_index(instr, 6)];
        let addr = base_addr.wrapping_add(offset);
        let op = instr.bits(10..12);

        if instr.bit(9) {
            // Thumb.8
            match op {
                // STRH Rd,[Rb,Ro]
                0 => Self::op_strh(bus, addr, self.reg.r[r] as u16),
                // LDSB Rd,[Rb,Ro]
                1 => self.reg.r[r] = Self::op_ldrb_or_ldsb(bus, addr, true),
                // LDRH/LDSH Rd,[Rb,Ro]
                2 | 3 => self.reg.r[r] = Self::op_ldrh_or_ldsh(bus, addr, op == 3),
                _ => unreachable!(),
            }
        } else {
            // Thumb.7
            match op {
                // STR Rd,[Rb,Ro]
                0 => Self::op_str(bus, addr, self.reg.r[r]),
                // STRB Rd,[Rb,Ro]
                1 => Self::op_strb(bus, addr, self.reg.r[r] as u8),
                // LDR Rd,[Rb,Ro]
                2 => self.reg.r[r] = Self::op_ldr(bus, addr),
                // LDRB Rd,[Rb,Ro]
                3 => self.reg.r[r] = Self::op_ldrb_or_ldsb(bus, addr, false),
                _ => unreachable!(),
            }
        }
    }

    /// Thumb.9: Load or store with immediate offset.
    fn execute_thumb9(&mut self, bus: &mut impl Bus, instr: u16) {
        let r = r_index(instr, 0);
        let base_addr = self.reg.r[r_index(instr, 3)];
        let offset = instr.bits(6..11).into();
        let addr = base_addr.wrapping_add(offset);
        let word_addr = base_addr.wrapping_add(offset * 4);

        match instr.bits(11..13) {
            // STR Rd,[Rb,#nn]
            0 => Self::op_str(bus, word_addr, self.reg.r[r]),
            // LDR Rd,[Rb,#nn]
            1 => self.reg.r[r] = Self::op_ldr(bus, word_addr),
            // STRB Rd,[Rb,#nn]
            #[allow(clippy::cast_possible_truncation)]
            2 => Self::op_strb(bus, addr, self.reg.r[r] as u8),
            // LDRB Rd,[Rb,#nn]
            3 => self.reg.r[r] = Self::op_ldrb_or_ldsb(bus, addr, false),
            _ => unreachable!(),
        }
    }

    /// Thumb.10: Load or store half-word.
    fn execute_thumb10(&mut self, bus: &mut impl Bus, instr: u16) {
        let r = r_index(instr, 0);
        let base_addr = self.reg.r[r_index(instr, 3)];
        let offset = u32::from(instr.bits(6..11));
        let addr = base_addr.wrapping_add(offset * 2);

        if instr.bit(11) {
            // LDRH Rd,[Rb,#nn]
            self.reg.r[r] = Self::op_ldrh_or_ldsh(bus, addr, false);
        } else {
            // STRH Rd,[Rb,#nn]
            #[allow(clippy::cast_possible_truncation)]
            Self::op_strh(bus, addr, self.reg.r[r] as u16);
        }
    }

    /// Thumb.11: Load or store SP relative.
    fn execute_thumb11(&mut self, bus: &mut impl Bus, instr: u16) {
        let offset = u32::from(instr.bits(..8));
        let addr = self.reg.r[SP_INDEX].wrapping_add(offset * 4);
        let r = r_index(instr, 8);

        if instr.bit(11) {
            // LDR Rd,[SP,#nn]
            self.reg.r[r] = Self::op_ldr(bus, addr);
        } else {
            // STR Rd,[SP,#nn]
            Self::op_str(bus, addr, self.reg.r[r]);
        }
    }

    /// Thumb.12: Get relative address.
    fn execute_thumb12(&mut self, instr: u16) {
        let offset = u32::from(instr.bits(..8));
        let base_addr = self.reg.r[if instr.bit(11) { SP_INDEX } else { PC_INDEX }];

        // ADD Rd,(PC/SP),#nn
        self.reg.r[r_index(instr, 8)] = self.op_add(false, base_addr, offset);
    }

    /// Thumb.13: Add offset to SP.
    fn execute_thumb13(&mut self, instr: u16) {
        let offset = u32::from(instr.bits(..7)) * 4;

        self.reg.r[SP_INDEX] = if instr.bit(7) {
            // SUB SP,#nn
            self.op_sub(false, self.reg.r[SP_INDEX], offset)
        } else {
            // ADD SP,#nn
            self.op_add(false, self.reg.r[SP_INDEX], offset)
        };
    }

    /// Thumb.14: Push or pop registers.
    fn execute_thumb14(&mut self, bus: &mut impl Bus, instr: u16) {
        let pop = instr.bit(11);
        let flags = BlockTransferFlags {
            preindex: !pop,
            ascend: pop,
            load_psr_or_force_user: false,
            writeback: true,
        };

        let r_list_extra = if pop { PC_INDEX } else { LR_INDEX };
        #[allow(clippy::cast_possible_truncation)]
        let r_list = u16::from(instr as u8).with_bit(r_list_extra, instr.bit(8));

        if pop {
            // POP {Rlist}{PC} (LDMFD)
            self.op_ldm(bus, &flags, SP_INDEX, r_list);
        } else {
            // PUSH {Rlist}{LR} (STMFD)
            self.op_stm(bus, &flags, SP_INDEX, r_list);
        }
    }

    /// Thumb.15: Multiple load or store.
    fn execute_thumb15(&mut self, bus: &mut impl Bus, instr: u16) {
        #[allow(clippy::cast_possible_truncation)]
        let r_list = (instr as u8).into();
        let r_base = r_index(instr, 8);

        let flags = BlockTransferFlags {
            preindex: false,
            ascend: true,
            load_psr_or_force_user: false,
            writeback: true,
        };

        if instr.bit(11) {
            // LDMIA Rb!,{Rlist}
            self.op_ldm(bus, &flags, r_base, r_list);
        } else {
            // STMIA Rb!,{Rlist}
            self.op_stm(bus, &flags, r_base, r_list);
        }
    }

    /// Thumb.16: Conditional branch.
    #[allow(clippy::cast_possible_truncation)]
    fn execute_thumb16(&mut self, bus: &impl Bus, instr: u16) {
        if self.meets_condition(instr.bits(8..12) as u8) {
            // B{cond} label
            self.op_branch(bus, self.reg.r[PC_INDEX], 2 * i32::from(instr as i8));
        }
    }

    /// Thumb.18: Unconditional branch.
    fn execute_thumb18(&mut self, bus: &impl Bus, instr: u16) {
        // B label
        self.op_branch(
            bus,
            self.reg.r[PC_INDEX],
            2 * arbitrary_sign_extend!(i32, instr.bits(..11), 11),
        );
    }

    /// Thumb.19: Long branch with link.
    fn execute_thumb19(&mut self, bus: &impl Bus, instr: u16) {
        let hi_part = !instr.bit(11);
        let addr_offset_part = u32::from(instr.bits(..11));

        // BL label
        if hi_part {
            self.reg.r[LR_INDEX] = self.reg.r[PC_INDEX].wrapping_add(addr_offset_part << 12);
        } else {
            // Adjust for pipelining, which has us two instructions ahead.
            let return_addr = self.reg.r[PC_INDEX].wrapping_sub(self.reg.cpsr.state.instr_size());

            #[allow(clippy::cast_possible_wrap)]
            self.op_branch(bus, self.reg.r[LR_INDEX], (addr_offset_part << 1) as _);
            self.reg.r[LR_INDEX] = return_addr | 1; // bit 0 set indicates Thumb
        };
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
        arm7tdmi::{isa::tests::InstrTest, reg::LR_INDEX},
        bus::{tests::VecBus, BusExt},
    };

    use super::*;

    #[test]
    fn execute_thumb1() {
        // LSL{S} Rd,Rs,#Offset
        InstrTest::new_thumb(0b000_00_00011_001_100) // R4,R1,#3
            .setup(&|cpu| cpu.reg.r[1] = 0b10)
            .assert_r(1, 0b10)
            .assert_r(4, 0b10_000)
            .run();

        InstrTest::new_thumb(0b000_00_01111_111_000) // R0,R7,#15
            .setup(&|cpu| cpu.reg.r[7] = 1)
            .assert_r(0, 1 << 15)
            .assert_r(7, 1)
            .run();

        InstrTest::new_thumb(0b000_00_00001_111_000) // R0,R7,#1
            .setup(&|cpu| cpu.reg.r[7] = 1 << 31)
            .assert_r(7, 1 << 31)
            .assert_carry()
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b000_00_01010_111_000) // R0,R7,#10
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b000_00_00000_000_000) // R0,R0,#0
            .setup(&|cpu| cpu.reg.r[0] = u32::MAX)
            .assert_r(0, u32::MAX)
            .assert_signed()
            .run();

        // LSR{S} Rd,Rs,#Offset
        InstrTest::new_thumb(0b000_01_00011_001_100) // R4,R1,#2
            .setup(&|cpu| cpu.reg.r[1] = 0b100)
            .assert_r(1, 0b100)
            .assert_zero()
            .assert_carry()
            .run();

        InstrTest::new_thumb(0b000_01_00011_001_100) // R4,R1,#2
            .setup(&|cpu| cpu.reg.r[1] = 0b10)
            .assert_r(1, 0b10)
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b000_01_11111_111_111) // R7,R7,#31
            .setup(&|cpu| cpu.reg.r[7] = 1 << 31)
            .assert_r(7, 1)
            .run();

        InstrTest::new_thumb(0b000_01_00000_111_111) // R7,R7,#32
            .setup(&|cpu| cpu.reg.r[7] = 1 << 31)
            .assert_zero()
            .assert_carry()
            .run();

        // ASR{S} Rd,Rs,#Offset
        InstrTest::new_thumb(0b000_10_11111_111_111) // R7,R7,#31
            .setup(&|cpu| cpu.reg.r[7] = 1 << 31)
            .assert_r(7, u32::MAX)
            .assert_signed()
            .run();

        InstrTest::new_thumb(0b000_10_00001_101_000) // R0,R5,#1
            .setup(&|cpu| cpu.reg.r[5] = !(1 << 31))
            .assert_r(0, !(0b11 << 30))
            .assert_r(5, !(1 << 31))
            .assert_carry()
            .run();

        InstrTest::new_thumb(0b000_10_00000_111_111) // R7,R7,#32
            .setup(&|cpu| cpu.reg.r[7] = 1 << 31)
            .assert_r(7, u32::MAX)
            .assert_signed()
            .assert_carry()
            .run();
    }

    #[test]
    #[allow(clippy::cast_sign_loss)]
    fn execute_thumb2() {
        // ADD{S} Rd,Rs,Rn
        InstrTest::new_thumb(0b00011_00_111_001_100) // R4,R1,R7
            .setup(&|cpu| {
                cpu.reg.r[1] = 13;
                cpu.reg.r[7] = 7;
            })
            .assert_r(1, 13)
            .assert_r(4, 20)
            .assert_r(7, 7)
            .run();

        InstrTest::new_thumb(0b00011_00_111_111_111) // R7,R7,R7
            .setup(&|cpu| cpu.reg.r[7] = 1)
            .assert_r(7, 2)
            .run();

        InstrTest::new_thumb(0b00011_00_111_110_000) // R0,R6,R7
            .setup(&|cpu| {
                cpu.reg.r[6] = u32::MAX;
                cpu.reg.r[7] = 1;
            })
            .assert_r(6, u32::MAX)
            .assert_r(7, 1)
            .assert_carry()
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b00011_00_000_001_010) // R2,R1,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = -5 as _;
                cpu.reg.r[1] = -10 as _;
            })
            .assert_r(0, -5 as _)
            .assert_r(1, -10 as _)
            .assert_r(2, -15 as _)
            .assert_signed()
            .assert_carry()
            .run();

        InstrTest::new_thumb(0b00011_00_000_001_010) // R2,R1,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = i32::MIN as _;
                cpu.reg.r[1] = -1 as _;
            })
            .assert_r(0, i32::MIN as _)
            .assert_r(1, -1 as _)
            .assert_r(2, i32::MIN.wrapping_sub(1) as _)
            .assert_carry()
            .assert_overflow()
            .run();

        // SUB{S} Rd,Rs,Rn
        InstrTest::new_thumb(0b00011_01_110_011_000) // R0,R3,R6
            .setup(&|cpu| {
                cpu.reg.r[3] = i32::MIN as _;
                cpu.reg.r[6] = i32::MAX as _;
            })
            .assert_r(0, 1)
            .assert_r(3, i32::MIN as _)
            .assert_r(6, i32::MAX as _)
            .assert_carry()
            .assert_overflow()
            .run();

        InstrTest::new_thumb(0b00011_01_000_000_010) // R2,R0,R0
            .setup(&|cpu| cpu.reg.r[0] = -5 as _)
            .assert_r(0, -5 as _)
            .assert_carry()
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b00011_01_000_001_010) // R2,R1,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = 5;
                cpu.reg.r[1] = -10 as _;
            })
            .assert_r(0, 5)
            .assert_r(1, -10 as _)
            .assert_r(2, -15 as _)
            .assert_signed()
            .assert_carry()
            .run();

        InstrTest::new_thumb(0b00011_01_000_001_010) // R2,R1,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = 1;
                cpu.reg.r[1] = i32::MIN as u32 + 1;
            })
            .assert_r(0, 1)
            .assert_r(1, i32::MIN as u32 + 1)
            .assert_r(2, i32::MIN as _)
            .assert_signed()
            .assert_overflow()
            .assert_carry()
            .run();

        InstrTest::new_thumb(0b00011_01_000_001_010) // R2,R1,R0
            .setup(&|cpu| cpu.reg.r[0] = 1 << 31)
            .assert_r(0, 1 << 31)
            .assert_r(2, 1 << 31)
            .assert_signed()
            .assert_overflow()
            .run();

        // ADD{S} Rd,Rs,#nn
        InstrTest::new_thumb(0b00011_10_101_000_000) // R0,R0,#5
            .setup(&|cpu| cpu.reg.r[0] = 10)
            .assert_r(0, 15)
            .run();

        // SUB{S} Rd,Rs,#nn
        InstrTest::new_thumb(0b00011_11_010_000_000) // R0,R0,#2
            .setup(&|cpu| cpu.reg.r[0] = 10)
            .assert_r(0, 8)
            .assert_carry()
            .run();
    }

    #[test]
    fn execute_thumb3() {
        // MOV{S} Rd,#nn
        InstrTest::new_thumb(0b001_00_101_11111111) // R5,#255
            .setup(&|cpu| cpu.reg.cpsr.signed = true)
            .assert_r(5, 255)
            .run();

        InstrTest::new_thumb(0b001_00_001_00000000) // R1,#0
            .setup(&|cpu| cpu.reg.r[1] = 1337)
            .assert_zero()
            .run();

        // CMP{S} Rd,#nn
        InstrTest::new_thumb(0b001_01_110_11111111) // R6,#255
            .setup(&|cpu| cpu.reg.r[6] = 255)
            .assert_r(6, 255)
            .assert_zero()
            .assert_carry()
            .run();

        InstrTest::new_thumb(0b001_01_010_00000000) // R2,#0
            .setup(&|cpu| cpu.reg.r[2] = 13)
            .assert_r(2, 13)
            .assert_carry()
            .run();

        // ADD{S} Rd,#nn
        InstrTest::new_thumb(0b001_10_111_10101010) // R7,#170
            .setup(&|cpu| cpu.reg.r[7] = 3)
            .assert_r(7, 173)
            .run();

        // SUB{S} Rd,#nn
        #[allow(clippy::cast_sign_loss)]
        InstrTest::new_thumb(0b001_11_011_00001111) // R3,#15
            .setup(&|cpu| cpu.reg.r[3] = 10)
            .assert_r(3, -5 as _)
            .assert_signed()
            .run();
    }

    #[test]
    #[allow(clippy::cast_sign_loss)]
    fn execute_thumb4() {
        // AND{S} Rd,Rs
        InstrTest::new_thumb(0b010000_0000_001_000) // R0,R1
            .setup(&|cpu| {
                cpu.reg.r[0] = 0b0011;
                cpu.reg.r[1] = 0b1010;
            })
            .assert_r(0, 0b0010)
            .assert_r(1, 0b1010)
            .run();

        InstrTest::new_thumb(0b010000_0000_001_000) // R0,R1
            .setup(&|cpu| cpu.reg.r[1] = 0b1010)
            .assert_r(1, 0b1010)
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b010000_0000_101_001) // R1,R5
            .setup(&|cpu| {
                cpu.reg.r[1] = i32::MIN as _;
                cpu.reg.r[5] = 1 << 31;
            })
            .assert_r(1, i32::MIN as _)
            .assert_r(5, 1 << 31)
            .assert_signed()
            .run();

        // EOR{S} Rd,Rs
        InstrTest::new_thumb(0b010000_0001_001_000) // R0,R1
            .setup(&|cpu| {
                cpu.reg.r[0] = 0b0011;
                cpu.reg.r[1] = 0b1110;
            })
            .assert_r(0, 0b1101)
            .assert_r(1, 0b1110)
            .run();

        InstrTest::new_thumb(0b010000_0001_000_001) // R1,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = 0b1100;
                cpu.reg.r[1] = 0b1100;
            })
            .assert_r(0, 0b1100)
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b010000_0001_001_111) // R7,R1
            .setup(&|cpu| {
                cpu.reg.r[1] = u32::MAX;
                cpu.reg.r[7] = u32::MAX >> 1;
            })
            .assert_r(1, u32::MAX)
            .assert_r(7, 1 << 31)
            .assert_signed()
            .run();

        // LSL{S} Rd,Rs
        // this test should not panic due to shift overflow:
        InstrTest::new_thumb(0b010000_0010_001_111) // R7,R1
            .setup(&|cpu| {
                cpu.reg.r[1] = 32;
                cpu.reg.r[7] = 1;
            })
            .assert_r(1, 32)
            .assert_zero()
            .assert_carry()
            .run();

        InstrTest::new_thumb(0b010000_0010_001_111) // R7,R1
            .setup(&|cpu| {
                cpu.reg.r[1] = 33;
                cpu.reg.r[7] = 1;
            })
            .assert_r(1, 33)
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b010000_0010_001_111) // R7,R1
            .setup(&|cpu| {
                cpu.reg.r[1] = u8::MAX.into();
                cpu.reg.r[7] = 1;
            })
            .assert_r(1, u8::MAX.into())
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b010000_0010_001_111) // R7,R1
            .setup(&|cpu| cpu.reg.r[7] = 1 << 31)
            .assert_r(7, 1 << 31)
            .assert_signed()
            .run();

        // LSR{S} Rd,Rs
        // this test should not panic due to shift overflow:
        InstrTest::new_thumb(0b010000_0011_000_001) // R1,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = 32;
                cpu.reg.r[1] = 1 << 31;
            })
            .assert_r(0, 32)
            .assert_zero()
            .assert_carry()
            .run();

        InstrTest::new_thumb(0b010000_0011_000_001) // R1,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = 33;
                cpu.reg.r[1] = 1 << 31;
            })
            .assert_r(0, 33)
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b010000_0011_000_001) // R1,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = u8::MAX.into();
                cpu.reg.r[1] = 1;
            })
            .assert_r(0, u8::MAX.into())
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b010000_0011_000_001) // R1,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = 3;
                cpu.reg.r[1] = 0b1000;
            })
            .assert_r(0, 3)
            .assert_r(1, 1)
            .run();

        InstrTest::new_thumb(0b010000_0011_001_111) // R7,R1
            .setup(&|cpu| cpu.reg.r[7] = 1)
            .assert_r(7, 1)
            .run();

        // ASR{S} Rd,Rs
        // this test should not panic due to shift overflow:
        InstrTest::new_thumb(0b010000_0100_001_000) // R0,R1
            .setup(&|cpu| {
                cpu.reg.r[0] = 1 << 31;
                cpu.reg.r[1] = 32;
            })
            .assert_r(0, u32::MAX)
            .assert_r(1, 32)
            .assert_signed()
            .assert_carry()
            .run();

        InstrTest::new_thumb(0b010000_0100_001_000) // R0,R1
            .setup(&|cpu| {
                cpu.reg.r[0] = 1 << 31;
                cpu.reg.r[1] = 33;
            })
            .assert_r(0, u32::MAX)
            .assert_r(1, 33)
            .assert_signed()
            .assert_carry()
            .run();

        InstrTest::new_thumb(0b010000_0100_001_000) // R0,R1
            .setup(&|cpu| {
                cpu.reg.r[0] = 1 << 31;
                cpu.reg.r[1] = u8::MAX.into();
            })
            .assert_r(0, u32::MAX)
            .assert_r(1, u8::MAX.into())
            .assert_signed()
            .assert_carry()
            .run();

        InstrTest::new_thumb(0b010000_0100_001_000) // R0,R1
            .setup(&|cpu| {
                cpu.reg.r[0] = 1 << 30;
                cpu.reg.r[1] = u8::MAX.into();
            })
            .assert_r(1, u8::MAX.into())
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b010000_0100_001_000) // R0,R1
            .setup(&|cpu| cpu.reg.r[0] = 1)
            .assert_r(0, 1)
            .run();

        // ADC{S} Rd,Rs
        InstrTest::new_thumb(0b010000_0101_000_001) // R1,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = 5;
                cpu.reg.r[1] = 32;
            })
            .assert_r(0, 5)
            .assert_r(1, 37)
            .run();

        InstrTest::new_thumb(0b010000_0101_000_001) // R1,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = 5;
                cpu.reg.r[1] = 32;
                cpu.reg.cpsr.carry = true;
            })
            .assert_r(0, 5)
            .assert_r(1, 38)
            .run();

        InstrTest::new_thumb(0b010000_0101_000_111) // R7,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[7] = 1;
            })
            .assert_r(0, u32::MAX)
            .assert_carry()
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b010000_0101_000_111) // R7,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[7] = 1;
                cpu.reg.cpsr.carry = true;
            })
            .assert_r(0, u32::MAX)
            .assert_r(7, 1)
            .assert_carry()
            .run();

        InstrTest::new_thumb(0b010000_0101_000_111) // R7,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[7] = u32::MAX;
            })
            .assert_r(0, u32::MAX)
            .assert_r(7, -2 as _)
            .assert_carry()
            .assert_signed()
            .run();

        InstrTest::new_thumb(0b010000_0101_000_111) // R7,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[7] = u32::MAX;
                cpu.reg.cpsr.carry = true;
            })
            .assert_r(0, u32::MAX)
            .assert_r(7, -1 as _)
            .assert_carry()
            .assert_signed()
            .run();

        InstrTest::new_thumb(0b010000_0101_000_111) // R7,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[7] = u32::MAX;
                cpu.reg.cpsr.carry = true;
            })
            .assert_r(0, u32::MAX)
            .assert_r(7, -1 as _)
            .assert_carry()
            .assert_signed()
            .run();

        InstrTest::new_thumb(0b010000_0101_000_111) // R7,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.cpsr.carry = true;
            })
            .assert_r(0, u32::MAX)
            .assert_carry()
            .assert_zero()
            .run();

        // SBC{S} Rd,Rs
        InstrTest::new_thumb(0b010000_0110_000_001) // R1,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = 5;
                cpu.reg.r[1] = 32;
            })
            .assert_r(0, 5)
            .assert_r(1, 26)
            .assert_carry()
            .run();

        InstrTest::new_thumb(0b010000_0110_000_001) // R1,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = 5;
                cpu.reg.r[1] = 32;
                cpu.reg.cpsr.carry = true;
            })
            .assert_r(0, 5)
            .assert_r(1, 27)
            .assert_carry()
            .run();

        InstrTest::new_thumb(0b010000_0110_000_111) // R7,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = -1 as _;
                cpu.reg.r[7] = 1;
            })
            .assert_r(0, u32::MAX)
            .assert_r(7, 1)
            .run();

        InstrTest::new_thumb(0b010000_0110_000_111) // R7,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = -1 as _;
                cpu.reg.r[7] = 1;
                cpu.reg.cpsr.carry = true;
            })
            .assert_r(0, u32::MAX)
            .assert_r(7, 2)
            .run();

        InstrTest::new_thumb(0b010000_0110_000_111) // R7,R0
            .setup(&|cpu| cpu.reg.r[7] = i32::MIN as _)
            .assert_r(7, i32::MAX as _)
            .assert_overflow()
            .assert_carry()
            .run();

        InstrTest::new_thumb(0b010000_0110_000_111) // R7,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = i32::MAX as _;
                cpu.reg.r[7] = i32::MIN as _;
            })
            .assert_r(0, i32::MAX as _)
            .assert_overflow()
            .assert_carry()
            .assert_zero()
            .run();

        // ROR{S} Rd,Rs
        InstrTest::new_thumb(0b010000_0111_000_001) // R1,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = 2;
                cpu.reg.r[1] = 0b1111;
            })
            .assert_r(0, 2)
            .assert_r(1, (0b11 << 30) | 0b11)
            .assert_carry()
            .assert_signed()
            .run();

        InstrTest::new_thumb(0b010000_0111_000_001) // R1,R0
            .setup(&|cpu| cpu.reg.r[1] = 0b1111)
            .assert_r(1, 0b1111)
            .run();

        InstrTest::new_thumb(0b010000_0111_010_011) // R3,R2
            .setup(&|cpu| {
                cpu.reg.r[2] = 255;
                cpu.reg.r[3] = 0b1111;
            })
            .assert_r(2, 255)
            .assert_r(3, 0b11110)
            .run();

        InstrTest::new_thumb(0b010000_0111_010_011) // R3,R2
            .setup(&|cpu| cpu.reg.r[2] = 255)
            .assert_r(2, 255)
            .assert_zero()
            .run();

        // TST Rd,Rs
        InstrTest::new_thumb(0b010000_1000_000_001) // R1,R0
            .setup(&|cpu| cpu.reg.r[1] = 0b1111)
            .assert_r(1, 0b1111)
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b010000_1000_000_001) // R1,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = 0b10000;
                cpu.reg.r[1] = 0b01111;
            })
            .assert_r(0, 0b10000)
            .assert_r(1, 0b01111)
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b010000_1000_000_001) // R1,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = 1;
                cpu.reg.r[1] = 1;
            })
            .assert_r(0, 1)
            .assert_r(1, 1)
            .run();

        InstrTest::new_thumb(0b010000_1000_000_001) // R1,R0
            .setup(&|cpu| {
                cpu.reg.r[0] = 1 << 31;
                cpu.reg.r[1] = u32::MAX;
            })
            .assert_r(0, 1 << 31)
            .assert_r(1, u32::MAX)
            .assert_signed()
            .run();

        // NEG{S} Rd,Rs
        InstrTest::new_thumb(0b010000_1001_011_111) // R7,R3
            .setup(&|cpu| cpu.reg.r[3] = 30)
            .assert_r(3, 30)
            .assert_r(7, -30 as _)
            .assert_signed()
            .run();

        InstrTest::new_thumb(0b010000_1001_011_111) // R7,R3
            .setup(&|cpu| cpu.reg.r[3] = 0)
            .assert_zero()
            .assert_carry()
            .run();

        InstrTest::new_thumb(0b010000_1001_011_111) // R7,R3
            .setup(&|cpu| cpu.reg.r[3] = -10 as _)
            .assert_r(3, -10 as _)
            .assert_r(7, 10)
            .run();

        // negating i32::MIN isn't possible, and it should also set the overflow flag
        InstrTest::new_thumb(0b010000_1001_011_111) // R7,R3
            .setup(&|cpu| cpu.reg.r[3] = i32::MIN as _)
            .assert_r(3, i32::MIN as _)
            .assert_r(7, i32::MIN as _)
            .assert_signed()
            .assert_overflow()
            .run();

        // CMP Rd,Rs
        InstrTest::new_thumb(0b010000_1010_011_100) // R4,R3
            .setup(&|cpu| {
                cpu.reg.r[3] = 30;
                cpu.reg.r[4] = 30;
            })
            .assert_r(3, 30)
            .assert_r(4, 30)
            .assert_zero()
            .assert_carry()
            .run();

        InstrTest::new_thumb(0b010000_1010_011_100) // R4,R3
            .setup(&|cpu| {
                cpu.reg.r[3] = 30;
                cpu.reg.r[4] = 20;
            })
            .assert_r(3, 30)
            .assert_r(4, 20)
            .assert_signed()
            .run();

        InstrTest::new_thumb(0b010000_1010_011_100) // R4,R3
            .setup(&|cpu| {
                cpu.reg.r[3] = 20;
                cpu.reg.r[4] = 30;
            })
            .assert_r(3, 20)
            .assert_r(4, 30)
            .assert_carry()
            .run();

        // CMN Rd,Rs
        InstrTest::new_thumb(0b010000_1011_011_100) // R4,R3
            .setup(&|cpu| {
                cpu.reg.r[3] = -30 as _;
                cpu.reg.r[4] = 30;
            })
            .assert_r(3, -30 as _)
            .assert_r(4, 30)
            .assert_zero()
            .assert_carry()
            .run();

        InstrTest::new_thumb(0b010000_1011_011_100) // R4,R3
            .setup(&|cpu| {
                cpu.reg.r[3] = -30 as _;
                cpu.reg.r[4] = 20;
            })
            .assert_r(3, -30 as _)
            .assert_r(4, 20)
            .assert_signed()
            .run();

        InstrTest::new_thumb(0b010000_1011_011_100) // R4,R3
            .setup(&|cpu| {
                cpu.reg.r[3] = -20 as _;
                cpu.reg.r[4] = 30;
            })
            .assert_r(3, -20 as _)
            .assert_r(4, 30)
            .assert_carry()
            .run();

        // ORR{S} Rd,Rs
        InstrTest::new_thumb(0b010000_1100_101_000) // R0,R5
            .setup(&|cpu| {
                cpu.reg.r[5] = 0b1010;
                cpu.reg.r[0] = 0b0101;
            })
            .assert_r(0, 0b1111)
            .assert_r(5, 0b1010)
            .run();

        InstrTest::new_thumb(0b010000_1100_101_000) // R0,R5
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b010000_1100_100_100) // R4,R4
            .setup(&|cpu| cpu.reg.r[4] = u32::MAX)
            .assert_r(4, u32::MAX)
            .assert_signed()
            .run();

        // MUL{S} Rd,Rs
        InstrTest::new_thumb(0b010000_1101_001_000) // R0,R1
            .setup(&|cpu| {
                cpu.reg.r[0] = 11;
                cpu.reg.r[1] = 3;
            })
            .assert_r(0, 33)
            .assert_r(1, 3)
            .run();

        InstrTest::new_thumb(0b010000_1101_001_000) // R0,R1
            .setup(&|cpu| {
                cpu.reg.r[0] = 0;
                cpu.reg.r[1] = 5;
            })
            .assert_r(1, 5)
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b010000_1101_001_000) // R0,R1
            .setup(&|cpu| {
                cpu.reg.r[0] = -8 as _;
                cpu.reg.r[1] = 14;
            })
            .assert_r(0, -112 as _)
            .assert_r(1, 14)
            .assert_signed()
            .run();

        InstrTest::new_thumb(0b010000_1101_001_000) // R0,R1
            .setup(&|cpu| {
                cpu.reg.r[0] = -4 as _;
                cpu.reg.r[1] = -4 as _;
            })
            .assert_r(0, 16)
            .assert_r(1, -4 as _)
            .run();

        // BIC{S} Rd,Rs
        InstrTest::new_thumb(0b010000_1110_001_000) // R0,R1
            .setup(&|cpu| {
                cpu.reg.r[0] = 0b11111;
                cpu.reg.r[1] = 0b10101;
            })
            .assert_r(0, 0b01010)
            .assert_r(1, 0b10101)
            .run();

        InstrTest::new_thumb(0b010000_1110_001_000) // R0,R1
            .setup(&|cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[1] = u32::MAX;
            })
            .assert_r(1, u32::MAX)
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b010000_1110_001_000) // R0,R1
            .setup(&|cpu| {
                cpu.reg.r[0] = u32::MAX;
                cpu.reg.r[1] = u32::MAX >> 1;
            })
            .assert_r(0, 1 << 31)
            .assert_r(1, u32::MAX >> 1)
            .assert_signed()
            .run();

        // MVN{S} Rd,Rs
        InstrTest::new_thumb(0b010000_1111_000_000) // R0,R0
            .setup(&|cpu| cpu.reg.r[0] = u32::MAX)
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b010000_1111_011_000) // R0,R3
            .setup(&|cpu| cpu.reg.r[3] = 0b1111_0000)
            .assert_r(0, !0b1111_0000)
            .assert_r(3, 0b1111_0000)
            .assert_signed()
            .run();
    }

    #[test]
    fn execute_thumb5() {
        // ADD Rd,Rs
        InstrTest::new_thumb(0b010001_00_1_0_001_101) // R13,R1
            .setup(&|cpu| {
                cpu.reg.r[13] = 20;
                cpu.reg.r[1] = 15;
            })
            .assert_r(1, 15)
            .assert_r(13, 35)
            .run();

        #[allow(clippy::cast_sign_loss)]
        InstrTest::new_thumb(0b010001_00_1_1_110_000) // R8,R14
            .setup(&|cpu| {
                cpu.reg.r[8] = 5;
                cpu.reg.r[14] = -10 as _;
            })
            .assert_r(8, -5 as _)
            .assert_r(14, -10 as _)
            .run();

        InstrTest::new_thumb(0b010001_00_1_1_010_111) // PC,R10
            .setup(&|cpu| {
                cpu.reg.r[PC_INDEX] = 1;
                cpu.reg.r[10] = 10;
            })
            .assert_r(10, 10)
            .assert_r(PC_INDEX, 14)
            .run();

        InstrTest::new_thumb(0b010001_00_1_1_010_111) // PC,R10
            .setup(&|cpu| {
                cpu.reg.r[PC_INDEX] = 0;
                cpu.reg.r[10] = 10;
            })
            .assert_r(10, 10)
            .assert_r(PC_INDEX, 14)
            .run();

        // CMP Rd,Rs
        InstrTest::new_thumb(0b010001_01_1_0_001_101) // R13,R1
            .setup(&|cpu| {
                cpu.reg.r[13] = 20;
                cpu.reg.r[1] = 15;
            })
            .assert_r(1, 15)
            .assert_r(13, 20)
            .assert_carry()
            .run();

        InstrTest::new_thumb(0b010001_01_0_1_101_001) // R1,R13
            .setup(&|cpu| {
                cpu.reg.r[13] = 20;
                cpu.reg.r[1] = 15;
            })
            .assert_r(1, 15)
            .assert_r(13, 20)
            .assert_signed()
            .run();

        InstrTest::new_thumb(0b010001_01_1_1_010_111) // PC,R10
            .setup(&|cpu| {
                cpu.reg.r[PC_INDEX] = 10;
                cpu.reg.r[10] = 10;
            })
            .assert_r(10, 10)
            .assert_r(PC_INDEX, 12)
            .assert_zero()
            .assert_carry()
            .run();

        // MOV Rd,Rs
        InstrTest::new_thumb(0b010001_10_1_0_001_101) // R13,R1
            .setup(&|cpu| cpu.reg.r[1] = 15)
            .assert_r(1, 15)
            .assert_r(13, 15)
            .run();

        InstrTest::new_thumb(0b010001_10_1_1_001_001) // R8,R8
            .setup(&|cpu| cpu.reg.r[8] = 15)
            .assert_r(8, 15)
            .run();

        // BX Rs
        let cpu = InstrTest::new_thumb(0b010001_11_1_0_001_101) // R1
            .setup(&|cpu| cpu.reg.r[1] = 0b111)
            .assert_r(1, 0b111)
            .assert_r(PC_INDEX, 0b110 + 4)
            .run();

        assert_eq!(cpu.reg.cpsr.state, OperationState::Thumb);

        let cpu = InstrTest::new_thumb(0b010001_11_0_1_101_000) // R13
            .setup(&|cpu| cpu.reg.r[13] = 0b110)
            .assert_r(13, 0b110)
            .assert_r(PC_INDEX, 0b100 + 8)
            .run();

        assert_eq!(cpu.reg.cpsr.state, OperationState::Arm);
    }

    #[test]
    fn execute_thumb6() {
        let mut bus = VecBus::new(88);
        bus.write_word(52, 0xdead_beef);
        bus.write_word(84, 0xbead_feed);

        // LDR Rd,[PC,#nn]
        InstrTest::new_thumb(0b01001_101_00001100) // R5,[PC,#48]
            .assert_r(5, 0xdead_beef)
            .run_with_bus(&mut bus);

        InstrTest::new_thumb(0b01001_000_00010000) // R0,[PC,#64]
            .setup(&|cpu| cpu.reg.r[PC_INDEX] = 20)
            .assert_r(0, 0xbead_feed)
            .assert_r(PC_INDEX, 22)
            .run_with_bus(&mut bus);
    }

    #[test]
    fn execute_thumb7() {
        let mut bus = VecBus::new(88);

        // STR Rd,[Rb,Ro]
        InstrTest::new_thumb(0b0101_00_0_010_001_000) // R0,[R1,R2]
            .setup(&|cpu| {
                cpu.reg.r[0] = 0xabcd_ef01;
                cpu.reg.r[1] = 10;
                cpu.reg.r[2] = 5;
            })
            .assert_r(0, 0xabcd_ef01)
            .assert_r(1, 10)
            .assert_r(2, 5)
            .run_with_bus(&mut bus);

        assert_eq!(0xabcd_ef01, bus.read_word(12));

        InstrTest::new_thumb(0b0101_00_0_010_001_000) // R0,[R1,R2]
            .setup(&|cpu| {
                cpu.reg.r[0] = 0x0102_abbc;
                cpu.reg.r[1] = 12;
                cpu.reg.r[2] = 4;
            })
            .assert_r(0, 0x0102_abbc)
            .assert_r(1, 12)
            .assert_r(2, 4)
            .run_with_bus(&mut bus);

        assert_eq!(0x0102_abbc, bus.read_word(16));

        // STRB Rd,[Rb,Ro]
        InstrTest::new_thumb(0b0101_01_0_010_001_000) // R0,[R1,R2]
            .setup(&|cpu| {
                cpu.reg.r[0] = 0xabab;
                cpu.reg.r[1] = 10;
                cpu.reg.r[2] = 9;
            })
            .assert_r(0, 0xabab)
            .assert_r(1, 10)
            .assert_r(2, 9)
            .run_with_bus(&mut bus);

        assert_eq!(0xab, bus.read_byte(19));
        assert_eq!(0, bus.read_byte(20));

        // LDR Rd,[Rb,Ro]
        InstrTest::new_thumb(0b0101_10_0_010_001_000) // R0,[R1,R2]
            .setup(&|cpu| {
                cpu.reg.r[1] = 4;
                cpu.reg.r[2] = 8;
            })
            .assert_r(0, 0xabcd_ef01)
            .assert_r(1, 4)
            .assert_r(2, 8)
            .run_with_bus(&mut bus);

        InstrTest::new_thumb(0b0101_10_0_010_001_000) // R0,[R1,R2]
            .setup(&|cpu| {
                cpu.reg.r[1] = 7;
                cpu.reg.r[2] = 8;
            })
            .assert_r(0, 0xcdef_01ab) // 3-byte misaligned read
            .assert_r(1, 7)
            .assert_r(2, 8)
            .run_with_bus(&mut bus);

        // LDRB Rd,[Rb,Ro]
        InstrTest::new_thumb(0b0101_11_0_110_001_000) // R0,[R1,R6]
            .setup(&|cpu| {
                cpu.reg.r[1] = 2;
                cpu.reg.r[6] = 17;
            })
            .assert_r(0, 0xab)
            .assert_r(1, 2)
            .assert_r(6, 17)
            .run_with_bus(&mut bus);
    }

    #[test]
    fn execute_thumb8() {
        let mut bus = VecBus::new(30);
        bus.write_byte(0, 0b0111_1110);
        bus.write_byte(21, !1);
        bus.write_hword(26, 1 << 15);

        // STRH Rd,[Rb,Ro]
        InstrTest::new_thumb(0b0101_00_1_010_001_000) // R0,[R1,R2]
            .setup(&|cpu| {
                cpu.reg.r[0] = 0xabcd_ef01;
                cpu.reg.r[1] = 10;
                cpu.reg.r[2] = 5;
            })
            .assert_r(0, 0xabcd_ef01)
            .assert_r(1, 10)
            .assert_r(2, 5)
            .run_with_bus(&mut bus);

        assert_eq!(0xef01, bus.read_hword(14));
        assert_eq!(0, bus.read_hword(16));

        // LDSB Rd,[Rb,Ro]
        InstrTest::new_thumb(0b0101_01_1_010_001_000) // R0,[R1,R2]
            .setup(&|cpu| {
                cpu.reg.r[1] = 20;
                cpu.reg.r[2] = 1;
            })
            .assert_r(0, u32::MAX.with_bit(0, false)) // sign-extended
            .assert_r(1, 20)
            .assert_r(2, 1)
            .run_with_bus(&mut bus);

        InstrTest::new_thumb(0b0101_01_1_010_001_000) // R0,[R1,R2]
            .assert_r(0, 0b0111_1110)
            .run_with_bus(&mut bus);

        // LDRH Rd,[Rb,Ro]
        InstrTest::new_thumb(0b0101_10_1_010_001_000) // R0,[R1,R2]
            .setup(&|cpu| {
                cpu.reg.r[1] = 13;
                cpu.reg.r[2] = 1;
            })
            .assert_r(0, 0xef01)
            .assert_r(1, 13)
            .assert_r(2, 1)
            .run_with_bus(&mut bus);

        // LDSH Rd,[Rb,Ro]
        InstrTest::new_thumb(0b0101_11_1_010_001_000) // R0,[R1,R2]
            .setup(&|cpu| {
                cpu.reg.r[1] = 2;
                cpu.reg.r[2] = 24;
            })
            .assert_r(0, u32::MAX.with_bits(..15, 0)) // sign-extended
            .assert_r(1, 2)
            .assert_r(2, 24)
            .run_with_bus(&mut bus);
    }

    #[test]
    fn execute_thumb9() {
        let mut bus = VecBus::new(40);

        // STR Rd,[Rb,#nn]
        InstrTest::new_thumb(0b011_00_00110_001_000) // R0,[R1,#24]
            .setup(&|cpu| {
                cpu.reg.r[0] = 0xabcd_ef01;
                cpu.reg.r[1] = 10;
            })
            .assert_r(0, 0xabcd_ef01)
            .assert_r(1, 10)
            .run_with_bus(&mut bus);

        assert_eq!(0xabcd_ef01, bus.read_word(32));

        // LDR Rd,[Rb,#nn]
        InstrTest::new_thumb(0b011_01_00110_001_000) // R0,[R1,#24]
            .setup(&|cpu| cpu.reg.r[1] = 8)
            .assert_r(0, 0xabcd_ef01)
            .assert_r(1, 8)
            .run_with_bus(&mut bus);

        // STRB Rd,[Rb,#nn]
        InstrTest::new_thumb(0b011_10_00110_001_000) // R0,[R1,#6]
            .setup(&|cpu| {
                cpu.reg.r[0] = 0xabcd_ef01;
                cpu.reg.r[1] = 10;
            })
            .assert_r(0, 0xabcd_ef01)
            .assert_r(1, 10)
            .run_with_bus(&mut bus);

        assert_eq!(0x01, bus.read_byte(16));

        // LDRB Rd,[Rb,#nn]
        InstrTest::new_thumb(0b011_11_00110_001_000) // R0,[R1,#6]
            .setup(&|cpu| cpu.reg.r[1] = 10)
            .assert_r(0, 0x01)
            .assert_r(1, 10)
            .run_with_bus(&mut bus);
    }

    #[test]
    fn execute_thumb10() {
        let mut bus = VecBus::new(40);

        // STRH Rd,[Rb,#nn]
        InstrTest::new_thumb(0b1000_0_00101_001_000) // R0,[R1,#10]
            .setup(&|cpu| {
                cpu.reg.r[0] = 0xabcd_ef01;
                cpu.reg.r[1] = 10;
            })
            .assert_r(0, 0xabcd_ef01)
            .assert_r(1, 10)
            .run_with_bus(&mut bus);

        assert_eq!(0xef01, bus.read_hword(20));

        // LDRH Rd,[Rb,#nn]
        InstrTest::new_thumb(0b1000_1_00110_001_000) // R0,[R1,#12]
            .setup(&|cpu| cpu.reg.r[1] = 8)
            .assert_r(0, 0xef01)
            .assert_r(1, 8)
            .run_with_bus(&mut bus);

        InstrTest::new_thumb(0b1000_1_00110_001_000) // R0,[R1,#12]
            .setup(&|cpu| cpu.reg.r[1] = 9) // Mis-aligned
            .assert_r(0, 0x0100_00ef)
            .assert_r(1, 9)
            .run_with_bus(&mut bus);
    }

    #[test]
    fn execute_thumb11() {
        let mut bus = VecBus::new(40);

        // STR Rd,[SP,#nn]
        InstrTest::new_thumb(0b1001_0_000_00000010) // R0,[SP,#8]
            .setup(&|cpu| {
                cpu.reg.r[SP_INDEX] = 8;
                cpu.reg.r[0] = 0xabcd_ef01;
            })
            .assert_r(0, 0xabcd_ef01)
            .assert_r(SP_INDEX, 8)
            .run_with_bus(&mut bus);

        assert_eq!(0xabcd_ef01, bus.read_word(16));

        // LDR Rd,[SP,#nn]
        InstrTest::new_thumb(0b1001_1_000_00000100) // R0,[SP,#16]
            .setup(&|cpu| cpu.reg.r[SP_INDEX] = 1)
            .assert_r(0, 0x01ab_cdef) // misaligned read by 1 byte
            .assert_r(SP_INDEX, 1)
            .run_with_bus(&mut bus);

        InstrTest::new_thumb(0b1001_1_000_00000001) // R0,[SP,#4]
            .setup(&|cpu| cpu.reg.r[SP_INDEX] = 12)
            .assert_r(0, 0xabcd_ef01)
            .assert_r(SP_INDEX, 12)
            .run_with_bus(&mut bus);
    }

    #[test]
    fn execute_thumb12() {
        // ADD Rd,[PC,#nn]
        InstrTest::new_thumb(0b1010_0_000_11001000) // R0,[PC,#200]
            .setup(&|cpu| cpu.reg.r[PC_INDEX] = 20)
            .assert_r(0, 220)
            .assert_r(PC_INDEX, 22)
            .run();

        InstrTest::new_thumb(0b1010_0_000_00000000) // R0,[PC,#0]
            .setup(&|cpu| cpu.reg.r[PC_INDEX] = 0)
            .assert_r(PC_INDEX, 2)
            .run();

        // ADD Rd,[SP,#nn]
        InstrTest::new_thumb(0b1010_1_000_11001000) // R0,[SP,#200]
            .setup(&|cpu| cpu.reg.r[SP_INDEX] = 40)
            .assert_r(0, 240)
            .assert_r(SP_INDEX, 40)
            .run();

        InstrTest::new_thumb(0b1010_1_000_00000000) // R0,[SP,#0]
            .run();
    }

    #[test]
    fn execute_thumb13() {
        // ADD SP,#nn
        InstrTest::new_thumb(0b10110000_0_0110010) // SP,#200
            .setup(&|cpu| cpu.reg.r[SP_INDEX] = 1)
            .assert_r(SP_INDEX, 201)
            .run();

        InstrTest::new_thumb(0b10110000_0_0000000) // SP,#0
            .run();

        // SUB SP,#nn
        InstrTest::new_thumb(0b10110000_1_0110010) // SP,#200
            .setup(&|cpu| cpu.reg.r[SP_INDEX] = 200)
            .run();

        InstrTest::new_thumb(0b10110000_1_0110010) // SP,#200
            .setup(&|cpu| cpu.reg.r[SP_INDEX] = 50)
            .assert_r(SP_INDEX, u32::MAX - 149)
            .run();
    }

    #[test]
    fn execute_thumb14() {
        let mut bus = VecBus::new(48);

        // PUSH {Rlist}{LR}
        InstrTest::new_thumb(0b1011_0_10_0_10001001) // {R0,R3,R7}
            .setup(&|cpu| {
                cpu.reg.r[SP_INDEX] = 41; // Mis-aligned SP.
                cpu.reg.r[0] = 0xabcd;
                cpu.reg.r[3] = 0xfefe_0001;
                cpu.reg.r[7] = 42;
            })
            .assert_r(0, 0xabcd)
            .assert_r(3, 0xfefe_0001)
            .assert_r(7, 42)
            .assert_r(SP_INDEX, 29)
            .run_with_bus(&mut bus);

        assert_eq!(42, bus.read_word(36));
        assert_eq!(0xfefe_0001, bus.read_word(32));
        assert_eq!(0xabcd, bus.read_word(28));

        InstrTest::new_thumb(0b1011_0_10_1_00000010) // {R1,LR}
            .setup(&|cpu| {
                cpu.reg.r[SP_INDEX] = 28;
                cpu.reg.r[1] = 0b1010;
                cpu.reg.r[LR_INDEX] = 40;
            })
            .assert_r(1, 0b1010)
            .assert_r(SP_INDEX, 20)
            .assert_r(LR_INDEX, 40)
            .run_with_bus(&mut bus);

        assert_eq!(40, bus.read_word(24));
        assert_eq!(0b1010, bus.read_word(20));

        // POP {Rlist}{PC}
        InstrTest::new_thumb(0b1011_1_10_1_00000001) // {R1,PC}
            .setup(&|cpu| cpu.reg.r[SP_INDEX] = 20)
            .assert_r(0, 0b1010)
            .assert_r(SP_INDEX, 28)
            .assert_r(PC_INDEX, 44)
            .run_with_bus(&mut bus);

        InstrTest::new_thumb(0b1011_1_10_0_10001001) // {R0,R3,R7}
            .setup(&|cpu| cpu.reg.r[SP_INDEX] = 31) // Mis-aligned SP.
            .assert_r(0, 0xabcd)
            .assert_r(3, 0xfefe_0001)
            .assert_r(7, 42)
            .assert_r(SP_INDEX, 43)
            .run_with_bus(&mut bus);
    }

    #[test]
    fn execute_thumb15() {
        let mut bus = VecBus::new(40);

        // STMIA Rb!,{Rlist}
        InstrTest::new_thumb(0b1100_0_101_10001001) // R5!,{R0,R3,R7}
            .setup(&|cpu| {
                cpu.reg.r[0] = 0xabcd;
                cpu.reg.r[3] = 0xfefe_0001;
                cpu.reg.r[5] = 20;
                cpu.reg.r[7] = 42;
            })
            .assert_r(0, 0xabcd)
            .assert_r(3, 0xfefe_0001)
            .assert_r(5, 32)
            .assert_r(7, 42)
            .run_with_bus(&mut bus);

        assert_eq!(0xabcd, bus.read_word(20));
        assert_eq!(0xfefe_0001, bus.read_word(24));
        assert_eq!(42, bus.read_word(28));

        InstrTest::new_thumb(0b1100_0_101_00000001) // R5!,{R0}
            .setup(&|cpu| {
                cpu.reg.r[0] = 0xbeef_fefe;
                cpu.reg.r[5] = 11; // Mis-aligned Rb.
            })
            .assert_r(0, 0xbeef_fefe)
            .assert_r(5, 15)
            .run_with_bus(&mut bus);

        assert_eq!(0xbeef_fefe, bus.read_word(8));

        // LDMIA Rb!,{Rlist}
        InstrTest::new_thumb(0b1100_1_101_10001001) // R5!,{R0,R3,R7}
            .setup(&|cpu| cpu.reg.r[5] = 20)
            .assert_r(0, 0xabcd)
            .assert_r(3, 0xfefe_0001)
            .assert_r(5, 32)
            .assert_r(7, 42)
            .run_with_bus(&mut bus);

        InstrTest::new_thumb(0b1100_1_101_00000001) // R5!,{R0}
            .setup(&|cpu| cpu.reg.r[5] = 11) // Mis-aligned Rb.
            .assert_r(0, 0xbeef_fefe)
            .assert_r(5, 15)
            .run_with_bus(&mut bus);
    }

    #[test]
    fn execute_thumb16() {
        // BEQ label
        InstrTest::new_thumb(0b1101_0000_00010100) // #40
            .setup(&|cpu| cpu.reg.cpsr.zero = true)
            .assert_r(PC_INDEX, 4 + 40 + 4)
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b1101_0000_00010100) // #40
            .run();

        // BNE label
        InstrTest::new_thumb(0b1101_0001_11101100) // #(-40)
            .assert_r(PC_INDEX, 4u32.wrapping_sub(40) + 4)
            .run();

        InstrTest::new_thumb(0b1101_0001_11101100) // #(-40)
            .setup(&|cpu| cpu.reg.cpsr.zero = true)
            .assert_zero()
            .run();

        // BCS/BHS label
        InstrTest::new_thumb(0b1101_0010_01111111) // #254
            .setup(&|cpu| cpu.reg.cpsr.carry = true)
            .assert_r(PC_INDEX, 4u32.wrapping_add(254) + 4)
            .assert_carry()
            .run();

        InstrTest::new_thumb(0b1101_0010_01111111) // #254
            .run();

        // BCC/BLO label
        InstrTest::new_thumb(0b1101_0011_10000000) // #(-256)
            .assert_r(PC_INDEX, 4u32.wrapping_sub(256) + 4)
            .run();

        InstrTest::new_thumb(0b1101_0011_10000000) // #(-256)
            .setup(&|cpu| cpu.reg.cpsr.carry = true)
            .assert_carry()
            .run();

        // BMI label
        InstrTest::new_thumb(0b1101_0100_00000000) // #0
            .setup(&|cpu| cpu.reg.cpsr.signed = true)
            .assert_r(PC_INDEX, 4 + 4)
            .assert_signed()
            .run();

        InstrTest::new_thumb(0b1101_0100_00000000) // #0
            .run();

        // BPL label
        InstrTest::new_thumb(0b1101_0101_00000010) // #4
            .assert_r(PC_INDEX, 4 + 4 + 4)
            .run();

        InstrTest::new_thumb(0b1101_0101_00000010) // #4
            .setup(&|cpu| cpu.reg.cpsr.signed = true)
            .assert_signed()
            .run();

        // BVS label
        InstrTest::new_thumb(0b1101_0110_11111101) // #(-6)
            .setup(&|cpu| cpu.reg.cpsr.overflow = true)
            .assert_r(PC_INDEX, 4u32.wrapping_sub(6).wrapping_add(4))
            .assert_overflow()
            .run();

        InstrTest::new_thumb(0b1101_0110_11111101) // #(-6)
            .run();

        // BVC label
        InstrTest::new_thumb(0b1101_0111_00000011) // #6
            .assert_r(PC_INDEX, 4 + 6 + 4)
            .run();

        InstrTest::new_thumb(0b1101_0111_00000011) // #6
            .setup(&|cpu| cpu.reg.cpsr.overflow = true)
            .assert_overflow()
            .run();

        // BHI label
        InstrTest::new_thumb(0b1101_1000_11111101) // #(-6)
            .setup(&|cpu| cpu.reg.cpsr.carry = true)
            .assert_r(PC_INDEX, 4u32.wrapping_sub(6).wrapping_add(4))
            .assert_carry()
            .run();

        InstrTest::new_thumb(0b1101_1000_11111101) // #(-6)
            .setup(&|cpu| {
                cpu.reg.cpsr.carry = true;
                cpu.reg.cpsr.zero = true;
            })
            .assert_carry()
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b1101_1000_11111101) // #(-6)
            .run();

        // BLS label
        InstrTest::new_thumb(0b1101_1001_11111101) // #(-6)
            .setup(&|cpu| cpu.reg.cpsr.carry = true)
            .assert_carry()
            .run();

        InstrTest::new_thumb(0b1101_1001_11111101) // #(-6)
            .setup(&|cpu| {
                cpu.reg.cpsr.carry = true;
                cpu.reg.cpsr.zero = true;
            })
            .assert_r(PC_INDEX, 4u32.wrapping_sub(6).wrapping_add(4))
            .assert_carry()
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b1101_1001_11111101) // #(-6)
            .assert_r(PC_INDEX, 4u32.wrapping_sub(6).wrapping_add(4))
            .run();

        // BGE label
        InstrTest::new_thumb(0b1101_1010_00000011) // #6
            .setup(&|cpu| {
                cpu.reg.cpsr.signed = true;
                cpu.reg.cpsr.overflow = true;
            })
            .assert_r(PC_INDEX, 4 + 6 + 4)
            .assert_signed()
            .assert_overflow()
            .run();

        InstrTest::new_thumb(0b1101_1010_00000011) // #6
            .assert_r(PC_INDEX, 4 + 6 + 4)
            .run();

        InstrTest::new_thumb(0b1101_1010_00000011) // #6
            .setup(&|cpu| cpu.reg.cpsr.overflow = true)
            .assert_overflow()
            .run();

        // BLT label
        InstrTest::new_thumb(0b1101_1011_00000011) // #6
            .setup(&|cpu| {
                cpu.reg.cpsr.signed = true;
                cpu.reg.cpsr.overflow = true;
            })
            .assert_signed()
            .assert_overflow()
            .run();

        InstrTest::new_thumb(0b1101_1011_00000011) // #6
            .run();

        InstrTest::new_thumb(0b1101_1011_00000011) // #6
            .setup(&|cpu| cpu.reg.cpsr.signed = true)
            .assert_r(PC_INDEX, 4 + 6 + 4)
            .assert_signed()
            .run();

        // BGT label
        InstrTest::new_thumb(0b1101_1100_00000011) // #6
            .setup(&|cpu| {
                cpu.reg.cpsr.signed = true;
                cpu.reg.cpsr.overflow = true;
            })
            .assert_r(PC_INDEX, 4 + 6 + 4)
            .assert_signed()
            .assert_overflow()
            .run();

        InstrTest::new_thumb(0b1101_1100_00000011) // #6
            .setup(&|cpu| cpu.reg.cpsr.zero = true)
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b1101_1100_00000011) // #6
            .assert_r(PC_INDEX, 4 + 6 + 4)
            .run();

        // BLE label
        InstrTest::new_thumb(0b1101_1101_00000011) // #6
            .setup(&|cpu| {
                cpu.reg.cpsr.signed = true;
                cpu.reg.cpsr.overflow = true;
            })
            .assert_signed()
            .assert_overflow()
            .run();

        InstrTest::new_thumb(0b1101_1101_00000011) // #6
            .setup(&|cpu| cpu.reg.cpsr.zero = true)
            .assert_r(PC_INDEX, 4 + 6 + 4)
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b1101_1101_00000011) // #6
            .run();
    }

    #[test]
    fn execute_thumb17() {
        // SWI nn
        InstrTest::new_thumb(0b11011111_10101010)
            .setup(&|cpu| {
                cpu.reg.r[PC_INDEX] = 200;
                cpu.reg.cpsr.irq_disabled = false;
            })
            .assert_r(LR_INDEX, 196)
            .assert_r(PC_INDEX, 0x08 + 8)
            .run();
    }

    #[test]
    fn execute_thumb18() {
        // B label
        InstrTest::new_thumb(0b11100_00000010100) // #40
            .setup(&|cpu| cpu.reg.cpsr.zero = true)
            .assert_r(PC_INDEX, 4 + 40 + 4)
            .assert_zero()
            .run();

        InstrTest::new_thumb(0b11100_11111111111) // #(-2)
            .assert_r(PC_INDEX, 4 - 2 + 4)
            .run();

        InstrTest::new_thumb(0b11100_01111111111) // #2046
            .setup(&|cpu| {
                cpu.reg.cpsr.signed = true;
                cpu.reg.cpsr.zero = true;
                cpu.reg.cpsr.carry = true;
                cpu.reg.cpsr.overflow = true;
            })
            .assert_r(PC_INDEX, 4 + 2046 + 4)
            .assert_signed()
            .assert_zero()
            .assert_carry()
            .assert_overflow()
            .run();
    }

    #[test]
    fn execute_thumb19() {
        // BL label
        InstrTest::new_thumb(0b11110_00000010100) // #14000h (hi part)
            .assert_r(LR_INDEX, 0x14000 + 4)
            .run();

        InstrTest::new_thumb(0b11111_11111111111) // #FFEh (lo part)
            .setup(&|cpu| cpu.reg.r[LR_INDEX] = 0x14004)
            .assert_r(LR_INDEX, 3)
            .assert_r(PC_INDEX, 0x14004 + 0xffe + 4)
            .run();
    }
}
