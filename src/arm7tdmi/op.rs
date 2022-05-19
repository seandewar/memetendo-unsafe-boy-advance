use crate::bus::DataBus;

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
    // Using overflowing_neg(), check if b == i32::MIN. If it is, we'll overflow negating it here
    // (-i32::MIN == i32::MIN in 2s complement!), so make sure the overflow flag is set after.
    // c is our implementation detail, so an overflow in c is our fault and isn't handled here.
    let (b_neg, overflow) = (b as i32).overflowing_neg();
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

    /// NOTE: also reloads the pipeline.
    pub(super) fn execute_bx(&mut self, bus: &impl DataBus, pc: u32) {
        self.reg.cpsr.state = if pc & 1 == 0 {
            OperationState::Thumb
        } else {
            OperationState::Arm
        };
        self.reg.r[PC_INDEX] = pc;
        self.reload_pipeline(bus);
    }

    pub(super) fn execute_str(bus: &mut impl DataBus, addr: u32, value: u32) {
        bus.write_word(addr & !0b11, value);
    }

    pub(super) fn execute_strh(bus: &mut impl DataBus, addr: u32, value: u16) {
        bus.write_hword(addr & !1, value);
    }

    pub(super) fn execute_strb(bus: &mut impl DataBus, addr: u32, value: u8) {
        bus.write_byte(addr, value);
    }

    pub(super) fn execute_ldr(bus: &impl DataBus, addr: u32) -> u32 {
        bus.read_word(addr & !0b11)
    }

    #[allow(clippy::cast_sign_loss)]
    pub(super) fn execute_ldrh_ldsh(bus: &impl DataBus, addr: u32, sign_extend: bool) -> u32 {
        let result = bus.read_hword(addr & !1);
        if sign_extend {
            i32::from(result) as _
        } else {
            result.into()
        }
    }

    #[allow(clippy::cast_sign_loss)]
    pub(super) fn execute_ldrb_ldsb(bus: &impl DataBus, addr: u32, sign_extend: bool) -> u32 {
        let result = bus.read_byte(addr);
        if sign_extend {
            i32::from(result) as _
        } else {
            result.into()
        }
    }

    pub(super) fn execute_push(&mut self, bus: &mut impl DataBus, mut r_list: u8, push_lr: bool) {
        // TODO: what about SP alignment? and should we emulate weird r_list behaviour when its 0?
        if push_lr {
            self.reg.r[SP_INDEX] = self.reg.r[SP_INDEX].wrapping_sub(4);
            bus.write_word(self.reg.r[SP_INDEX], self.reg.r[LR_INDEX]);
        }

        for r in (0..8).rev() {
            if r_list & (1 << 7) != 0 {
                self.reg.r[SP_INDEX] = self.reg.r[SP_INDEX].wrapping_sub(4);
                bus.write_word(self.reg.r[SP_INDEX], self.reg.r[r]);
            }

            r_list <<= 1;
        }
    }

    pub(super) fn execute_pop(&mut self, bus: &impl DataBus, mut r_list: u8, pop_pc: bool) {
        // TODO: what about SP alignment? and should we emulate weird r_list behaviour when its 0?
        for r in 0..8 {
            if r_list & 1 != 0 {
                self.reg.r[r] = bus.read_word(self.reg.r[SP_INDEX]);
                self.reg.r[SP_INDEX] = self.reg.r[SP_INDEX].wrapping_add(4);
            }

            r_list >>= 1;
        }

        if pop_pc {
            self.reg.r[PC_INDEX] = bus.read_word(self.reg.r[SP_INDEX]);
            self.reg.r[SP_INDEX] = self.reg.r[SP_INDEX].wrapping_add(4);
            self.reload_pipeline(bus);
        }
    }
}
