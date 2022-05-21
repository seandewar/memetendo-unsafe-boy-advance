use crate::bus::{Bus, BusAlignedExt};

use super::{
    reg::{LR_INDEX, PC_INDEX, SP_INDEX},
    Cpu, OperationState,
};

#[allow(clippy::cast_sign_loss, clippy::cast_possible_wrap)]
fn execute_add_impl(cpu: &mut Cpu, update_cond: bool, a: u32, b: u32, c: u32) -> u32 {
    let (a_b, a_b_overflow) = (a as i32).overflowing_add(b as _);
    let (result, a_b_c_overflow) = a_b.overflowing_add(c as _);

    if update_cond {
        let actual_result = i64::from(a) + i64::from(b) + i64::from(c);
        cpu.reg.cpsr.overflow = a_b_overflow || a_b_c_overflow;
        cpu.reg.cpsr.carry = actual_result as u64 > u32::MAX.into();
        cpu.reg.cpsr.set_nz_from(result as _);
    }

    result as _
}

#[allow(clippy::cast_sign_loss, clippy::cast_possible_wrap)]
fn execute_sub_impl(cpu: &mut Cpu, update_cond: bool, a: u32, b: u32, c: u32) -> u32 {
    let (b_neg, overflow) = (b as i32).overflowing_neg();

    // c is our implementation detail; it's not expected to overflow.
    let result = execute_add_impl(cpu, update_cond, a, b_neg as _, -(c as i32) as _);
    cpu.reg.cpsr.overflow |= update_cond && overflow;

    result
}

impl Cpu {
    pub(super) fn execute_add_cmn(&mut self, update_cond: bool, a: u32, b: u32) -> u32 {
        execute_add_impl(self, update_cond, a, b, 0)
    }

    #[allow(clippy::cast_sign_loss, clippy::cast_possible_wrap)]
    pub(super) fn execute_sub_cmp(&mut self, update_cond: bool, a: u32, b: u32) -> u32 {
        execute_sub_impl(self, update_cond, a, b, 0)
    }

    pub(super) fn execute_adc(&mut self, update_cond: bool, a: u32, b: u32) -> u32 {
        execute_add_impl(self, update_cond, a, b, self.reg.cpsr.carry.into())
    }

    pub(super) fn execute_sbc(&mut self, update_cond: bool, a: u32, b: u32) -> u32 {
        execute_sub_impl(self, update_cond, a, b, (!self.reg.cpsr.carry).into())
    }

    pub(super) fn execute_mul(&mut self, a: u32, b: u32) -> u32 {
        let result = a.wrapping_mul(b);
        self.reg.cpsr.set_nz_from(result); // TODO: MUL corrupts carry flag (lol), but how?

        result
    }

    pub(super) fn execute_mov(&mut self, update_cond: bool, value: u32) -> u32 {
        if update_cond {
            self.reg.cpsr.set_nz_from(value);
        }

        value
    }

    pub(super) fn execute_and_tst(&mut self, a: u32, b: u32) -> u32 {
        let result = a & b;
        self.reg.cpsr.set_nz_from(result);

        result
    }

    pub(super) fn execute_bic(&mut self, a: u32, b: u32) -> u32 {
        let result = a & !b;
        self.reg.cpsr.set_nz_from(result);

        result
    }

    pub(super) fn execute_eor(&mut self, a: u32, b: u32) -> u32 {
        let result = a ^ b;
        self.reg.cpsr.set_nz_from(result);

        result
    }

    pub(super) fn execute_orr(&mut self, a: u32, b: u32) -> u32 {
        let result = a | b;
        self.reg.cpsr.set_nz_from(result);

        result
    }

    pub(super) fn execute_mvn(&mut self, value: u32) -> u32 {
        let result = !value;
        self.reg.cpsr.set_nz_from(result);

        result
    }

    pub(super) fn execute_lsl(&mut self, value: u32, offset: u8) -> u32 {
        let mut result = value;
        if offset > 0 {
            result = result.checked_shl((offset - 1).into()).unwrap_or(0);
            self.reg.cpsr.carry = result & (1 << 31) != 0;
            result <<= 1;
        }
        self.reg.cpsr.set_nz_from(result);

        result
    }

    pub(super) fn execute_lsr(&mut self, value: u32, offset: u8) -> u32 {
        // LSR/ASR #0 is a special case that works like LSR/ASR #32
        let offset = if offset == 0 { 32 } else { offset.into() };

        let mut result = value;
        result = result.checked_shr(offset - 1).unwrap_or(0);
        self.reg.cpsr.carry = result & 1 != 0;
        result >>= 1;
        self.reg.cpsr.set_nz_from(result);

        result
    }

    #[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
    pub(super) fn execute_asr(&mut self, value: u32, offset: u8) -> u32 {
        // LSR/ASR #0 is a special case that works like LSR/ASR #32
        let offset = if offset == 0 { 32 } else { offset.into() };

        // A value shifted 32 or more times is either 0 or has all bits set depending on the
        // initial value of the sign bit (due to sign extension)
        let mut result = value as i32;
        let overflow_result = if result.is_negative() {
            u32::MAX as _
        } else {
            0
        };

        result = result.checked_shr(offset - 1).unwrap_or(overflow_result);
        self.reg.cpsr.carry = result & 1 != 0;
        let result = (result >> 1) as _;
        self.reg.cpsr.set_nz_from(result);

        result
    }

    pub(super) fn execute_ror(&mut self, value: u32, offset: u8) -> u32 {
        let mut result = value;
        if offset > 0 {
            result = value.rotate_right(u32::from(offset) - 1);
            self.reg.cpsr.carry = result & 1 != 0;
            result = result.rotate_right(1);
        }
        self.reg.cpsr.set_nz_from(result);

        result
    }

    pub(super) fn execute_bx(&mut self, bus: &impl Bus, addr: u32) {
        self.reg.cpsr.state = if addr & 1 == 0 {
            OperationState::Arm
        } else {
            OperationState::Thumb
        };
        self.reg.r[PC_INDEX] = addr;
        self.reload_pipeline(bus);
    }

    pub(super) fn execute_str(bus: &mut impl Bus, addr: u32, value: u32) {
        bus.write_word_aligned(addr, value);
    }

    pub(super) fn execute_strh(bus: &mut impl Bus, addr: u32, value: u16) {
        bus.write_hword_aligned(addr, value);
    }

    pub(super) fn execute_strb(bus: &mut impl Bus, addr: u32, value: u8) {
        bus.write_byte(addr, value);
    }

    pub(super) fn execute_ldr(bus: &impl Bus, addr: u32) -> u32 {
        bus.read_word_aligned(addr)
    }

    #[allow(clippy::cast_sign_loss)]
    pub(super) fn execute_ldrh_ldsh(bus: &impl Bus, addr: u32, sign_extend: bool) -> u32 {
        // TODO: emulate weird misaligned read behaviour? (LDRH Rd,[odd] -> LDRH Rd,[odd-1] ROR 8)
        let result = bus.read_hword_aligned(addr);

        if sign_extend {
            i32::from(result) as _
        } else {
            result.into()
        }
    }

    #[allow(clippy::cast_sign_loss)]
    pub(super) fn execute_ldrb_ldsb(bus: &impl Bus, addr: u32, sign_extend: bool) -> u32 {
        let result = bus.read_byte(addr);

        if sign_extend {
            i32::from(result) as _
        } else {
            result.into()
        }
    }

    pub(super) fn execute_stmia(&mut self, bus: &mut impl Bus, r_base_addr: usize, mut r_list: u8) {
        // TODO: emulate weird invalid r_list behaviour? (empty r_list, r_list with r_base_addr)
        for r in 0..8 {
            if r_list & 1 != 0 {
                bus.write_word_aligned(self.reg.r[r_base_addr], self.reg.r[r]);
                self.reg.r[r_base_addr] = self.reg.r[r_base_addr].wrapping_add(4);
            }

            r_list >>= 1;
        }
    }

    pub(super) fn execute_ldmia(&mut self, bus: &impl Bus, r_base_addr: usize, mut r_list: u8) {
        // TODO: emulate weird invalid r_list behaviour? (empty r_list, r_list with r_base_addr)
        for r in 0..8 {
            if r_list & 1 != 0 {
                self.reg.r[r] = bus.read_word_aligned(self.reg.r[r_base_addr]);
                self.reg.r[r_base_addr] = self.reg.r[r_base_addr].wrapping_add(4);
            }

            r_list >>= 1;
        }
    }

    pub(super) fn execute_push(&mut self, bus: &mut impl Bus, mut r_list: u8, push_lr: bool) {
        // TODO: emulate weird r_list behaviour when its 0?
        if push_lr {
            self.reg.r[SP_INDEX] = self.reg.r[SP_INDEX].wrapping_sub(4);
            bus.write_word_aligned(self.reg.r[SP_INDEX], self.reg.r[LR_INDEX]);
        }

        for r in (0..8).rev() {
            if r_list & (1 << 7) != 0 {
                self.reg.r[SP_INDEX] = self.reg.r[SP_INDEX].wrapping_sub(4);
                bus.write_word_aligned(self.reg.r[SP_INDEX], self.reg.r[r]);
            }

            r_list <<= 1;
        }
    }

    pub(super) fn execute_pop(&mut self, bus: &impl Bus, mut r_list: u8, pop_pc: bool) {
        // TODO: emulate weird r_list behaviour when its 0?
        for r in 0..8 {
            if r_list & 1 != 0 {
                self.reg.r[r] = bus.read_word_aligned(self.reg.r[SP_INDEX]);
                self.reg.r[SP_INDEX] = self.reg.r[SP_INDEX].wrapping_add(4);
            }

            r_list >>= 1;
        }

        if pop_pc {
            self.reg.r[PC_INDEX] = bus.read_word_aligned(self.reg.r[SP_INDEX]);
            self.reg.r[SP_INDEX] = self.reg.r[SP_INDEX].wrapping_add(4);
            self.reload_pipeline(bus);
        }
    }

    pub(super) fn execute_branch(&mut self, bus: &impl Bus, addr_offset: i16, cond: bool) {
        if cond {
            #[allow(clippy::cast_sign_loss)]
            let addr_offset = i32::from(addr_offset) as _;

            self.reg.r[PC_INDEX] = self.reg.r[PC_INDEX].wrapping_add(addr_offset);
            self.reload_pipeline(bus);
        }
    }

    pub(super) fn execute_bl(&mut self, bus: &impl Bus, hi_part: bool, addr_offset_part: u16) {
        let addr_offset_part = u32::from(addr_offset_part);

        if hi_part {
            self.reg.r[LR_INDEX] = self.reg.r[PC_INDEX].wrapping_add(addr_offset_part << 12);
        } else {
            // Adjust for pipelining, which has us two instructions ahead.
            let return_addr = self.reg.r[PC_INDEX].wrapping_sub(self.reg.cpsr.state.instr_size());

            self.reg.r[PC_INDEX] = self.reg.r[LR_INDEX].wrapping_add(addr_offset_part << 1);
            self.reg.r[LR_INDEX] = return_addr | 1; // OR 1 is used to indicate THUMB.
            self.reload_pipeline(bus);
        }
    }
}
