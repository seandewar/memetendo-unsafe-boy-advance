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

        match (instr.bits(25..28), instr.bits(24..28), instr.bits(8..28)) {
            (0b101, _, _) => self.execute_arm_b_bl(bus, instr),
            (_, _, 0b0001_0010_1111_1111_1111) => self.execute_arm_bx(bus, instr),
            // TODO: what happens if 0b0001? BKPT does apply to ARMv4
            (_, 0b1111, _) => self.enter_exception(bus, Exception::SoftwareInterrupt),
            (0b011, _, _) => self.enter_exception(bus, Exception::UndefinedInstr),
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
}

#[allow(clippy::unusual_byte_groupings)]
#[cfg(test)]
mod tests {
    use crate::arm7tdmi::{
        op::tests::InstrTest,
        reg::{OperationState, LR_INDEX, PC_INDEX},
    };

    #[test]
    fn execute_arm_branches() {
        // B{cond} label
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
            .assert_r(LR_INDEX, 8 - 4)
            .assert_r(PC_INDEX, 0x08 + 8)
            .assert_irq_disabled()
            .run();

        // Undefined
        InstrTest::new_arm(0b1110_011_01010101010101010101_1_1010) // AL
            .assert_r(LR_INDEX, 8 - 4)
            .assert_r(PC_INDEX, 0x04 + 8)
            .assert_irq_disabled()
            .run();
    }
}
