mod arm;
mod thumb;

use intbits::Bits;

use crate::bus::{Bus, BusAlignedExt};

use super::{
    reg::{OperationMode, StatusRegister, LR_INDEX, PC_INDEX, SP_INDEX},
    Cpu, OperationState,
};

impl StatusRegister {
    fn set_nz_from_word(&mut self, result: u32) {
        self.zero = result == 0;
        self.signed = result.bit(31);
    }

    fn set_nz_from_dword(&mut self, result: u64) {
        self.zero = result == 0;
        self.signed = result.bit(63);
    }
}

#[allow(clippy::cast_sign_loss, clippy::cast_possible_wrap)]
fn execute_add_impl(cpu: &mut Cpu, update_cond: bool, a: u32, b: u32, c: u32) -> u32 {
    let (a_b, a_b_overflow) = (a as i32).overflowing_add(b as _);
    let (result, a_b_c_overflow) = a_b.overflowing_add(c as _);

    if update_cond {
        let actual_result = i64::from(a) + i64::from(b) + i64::from(c);
        cpu.reg.cpsr.overflow = a_b_overflow || a_b_c_overflow;
        cpu.reg.cpsr.carry = actual_result as u64 > u32::MAX.into();
        cpu.reg.cpsr.set_nz_from_word(result as _);
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
    fn execute_add(&mut self, update_cond: bool, a: u32, b: u32) -> u32 {
        execute_add_impl(self, update_cond, a, b, 0)
    }

    fn execute_sub(&mut self, update_cond: bool, a: u32, b: u32) -> u32 {
        execute_sub_impl(self, update_cond, a, b, 0)
    }

    fn execute_adc(&mut self, update_cond: bool, a: u32, b: u32) -> u32 {
        execute_add_impl(self, update_cond, a, b, self.reg.cpsr.carry.into())
    }

    fn execute_sbc(&mut self, update_cond: bool, a: u32, b: u32) -> u32 {
        execute_sub_impl(self, update_cond, a, b, (!self.reg.cpsr.carry).into())
    }

    fn execute_mla(&mut self, update_cond: bool, a: u32, b: u32, accum: u32) -> u32 {
        let result = a.wrapping_mul(b).wrapping_add(accum);
        if update_cond {
            // TODO: should corrupt carry flag (lol), but how?
            self.reg.cpsr.set_nz_from_word(result);
        }

        result
    }

    fn execute_smlal(&mut self, update_cond: bool, a: i32, b: i32, accum: i64) -> u64 {
        #[allow(clippy::cast_sign_loss)]
        let result = i64::from(a).wrapping_mul(b.into()).wrapping_add(accum) as u64;
        if update_cond {
            // TODO: should corrupt carry flag (lol), but how?
            self.reg.cpsr.set_nz_from_dword(result);
        }

        result
    }

    fn execute_umlal(&mut self, update_cond: bool, a: u32, b: u32, accum: u64) -> u64 {
        let result = u64::from(a).wrapping_mul(b.into()).wrapping_add(accum);
        if update_cond {
            // TODO: should corrupt carry flag (lol), but how?
            self.reg.cpsr.set_nz_from_dword(result);
        }

        result
    }

    fn execute_mov(&mut self, update_cond: bool, value: u32) -> u32 {
        if update_cond {
            self.reg.cpsr.set_nz_from_word(value);
        }

        value
    }

    fn execute_and(&mut self, update_cond: bool, a: u32, b: u32) -> u32 {
        let result = a & b;
        if update_cond {
            self.reg.cpsr.set_nz_from_word(result);
        }

        result
    }

    fn execute_bic(&mut self, update_cond: bool, a: u32, b: u32) -> u32 {
        let result = a & !b;
        if update_cond {
            self.reg.cpsr.set_nz_from_word(result);
        }

        result
    }

    fn execute_eor(&mut self, update_cond: bool, a: u32, b: u32) -> u32 {
        let result = a ^ b;
        if update_cond {
            self.reg.cpsr.set_nz_from_word(result);
        }

        result
    }

    fn execute_orr(&mut self, update_cond: bool, a: u32, b: u32) -> u32 {
        let result = a | b;
        if update_cond {
            self.reg.cpsr.set_nz_from_word(result);
        }

        result
    }

    fn execute_mvn(&mut self, update_cond: bool, value: u32) -> u32 {
        let result = !value;
        if update_cond {
            self.reg.cpsr.set_nz_from_word(result);
        }

        result
    }

    fn execute_lsl(&mut self, update_cond: bool, value: u32, offset: u8) -> u32 {
        let mut result = value;
        if offset > 0 {
            result = result.checked_shl((offset - 1).into()).unwrap_or(0);
            if update_cond {
                self.reg.cpsr.carry = result.bit(31);
            }
            result <<= 1;
        }

        if update_cond {
            self.reg.cpsr.set_nz_from_word(result);
        }

        result
    }

    fn execute_lsr(
        &mut self,
        update_cond: bool,
        special_zero_offset: bool,
        value: u32,
        offset: u8,
    ) -> u32 {
        let offset = if special_zero_offset && offset == 0 {
            32 // #0 works like #32
        } else {
            offset.into()
        };

        let mut result = value;
        result = result.checked_shr(offset - 1).unwrap_or(0);
        if update_cond {
            self.reg.cpsr.carry = result.bit(0);
        }

        result >>= 1;
        if update_cond {
            self.reg.cpsr.set_nz_from_word(result);
        }

        result
    }

    #[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
    fn execute_asr(
        &mut self,
        update_cond: bool,
        special_zero_offset: bool,
        value: u32,
        offset: u8,
    ) -> u32 {
        let offset = if special_zero_offset && offset == 0 {
            32 // #0 works like #32
        } else {
            offset.into()
        };

        // A value shifted 32 or more times is either 0 or has all bits set depending on the
        // initial value of the sign bit (due to sign extension)
        let mut result = value as i32;
        let overflow_result = if result.is_negative() {
            u32::MAX as i32
        } else {
            0
        };

        result = result.checked_shr(offset - 1).unwrap_or(overflow_result);
        if update_cond {
            self.reg.cpsr.carry = result.bit(0);
        }

        let result = (result >> 1) as u32;
        if update_cond {
            self.reg.cpsr.set_nz_from_word(result);
        }

        result
    }

    fn execute_ror(
        &mut self,
        update_cond: bool,
        special_zero_offset: bool,
        value: u32,
        offset: u8,
    ) -> u32 {
        let mut result = value;

        if offset > 0 {
            result = value.rotate_right(u32::from(offset) - 1);
            if update_cond {
                self.reg.cpsr.carry = result.bit(0);
            }
            result = result.rotate_right(1);
        } else if special_zero_offset {
            // #0 works like RRX #1 (ROR #1, but bit 31 is set to the old carry)
            let old_carry = self.reg.cpsr.carry;
            if update_cond {
                self.reg.cpsr.carry = result.bit(0);
            }
            result = value.rotate_right(1);
            result.set_bit(31, old_carry);
        }

        if update_cond {
            self.reg.cpsr.set_nz_from_word(result);
        }

        result
    }

    fn execute_bx(&mut self, bus: &impl Bus, addr: u32) {
        self.reg.cpsr.state = if addr.bit(0) {
            OperationState::Thumb
        } else {
            OperationState::Arm
        };
        self.reg.r[PC_INDEX] = addr;
        self.reload_pipeline(bus);
    }

    fn execute_str(bus: &mut impl Bus, addr: u32, value: u32) {
        bus.write_word_aligned(addr, value);
    }

    fn execute_strh(bus: &mut impl Bus, addr: u32, value: u16) {
        bus.write_hword_aligned(addr, value);
    }

    fn execute_strb(bus: &mut impl Bus, addr: u32, value: u8) {
        bus.write_byte(addr, value);
    }

    fn execute_ldr(bus: &impl Bus, addr: u32) -> u32 {
        bus.read_word_aligned(addr)
    }

    #[allow(clippy::cast_sign_loss, clippy::cast_possible_wrap)]
    fn execute_ldrh_or_ldsh(bus: &impl Bus, addr: u32, sign_extend: bool) -> u32 {
        // TODO: emulate weird misaligned read behaviour? (LDRH Rd,[odd] -> LDRH Rd,[odd-1] ROR 8)
        let result = bus.read_hword_aligned(addr);

        if sign_extend {
            i32::from(result as i16) as _
        } else {
            result.into()
        }
    }

    #[allow(clippy::cast_sign_loss, clippy::cast_possible_wrap)]
    fn execute_ldrb_or_ldsb(bus: &impl Bus, addr: u32, sign_extend: bool) -> u32 {
        let result = bus.read_byte(addr);

        if sign_extend {
            i32::from(result as i8) as _
        } else {
            result.into()
        }
    }

    fn execute_stmia(&mut self, bus: &mut impl Bus, r_base_addr: usize, mut r_list: u8) {
        // TODO: emulate weird invalid r_list behaviour? (empty r_list, r_list with r_base_addr)
        for r in 0..8 {
            if r_list.bit(0) {
                bus.write_word_aligned(self.reg.r[r_base_addr], self.reg.r[r]);
                self.reg.r[r_base_addr] = self.reg.r[r_base_addr].wrapping_add(4);
            }
            r_list >>= 1;
        }
    }

    fn execute_ldmia(&mut self, bus: &impl Bus, r_base_addr: usize, mut r_list: u8) {
        // TODO: emulate weird invalid r_list behaviour? (empty r_list, r_list with r_base_addr)
        for r in 0..8 {
            if r_list.bit(0) {
                self.reg.r[r] = bus.read_word_aligned(self.reg.r[r_base_addr]);
                self.reg.r[r_base_addr] = self.reg.r[r_base_addr].wrapping_add(4);
            }
            r_list >>= 1;
        }
    }

    fn execute_push(&mut self, bus: &mut impl Bus, mut r_list: u8, push_lr: bool) {
        // TODO: emulate weird r_list behaviour when its 0?
        if push_lr {
            self.reg.r[SP_INDEX] = self.reg.r[SP_INDEX].wrapping_sub(4);
            bus.write_word_aligned(self.reg.r[SP_INDEX], self.reg.r[LR_INDEX]);
        }

        for r in (0..8).rev() {
            if r_list.bit(7) {
                self.reg.r[SP_INDEX] = self.reg.r[SP_INDEX].wrapping_sub(4);
                bus.write_word_aligned(self.reg.r[SP_INDEX], self.reg.r[r]);
            }
            r_list <<= 1;
        }
    }

    fn execute_pop(&mut self, bus: &impl Bus, mut r_list: u8, pop_pc: bool) {
        // TODO: emulate weird r_list behaviour when its 0?
        for r in 0..8 {
            if r_list.bit(0) {
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

    #[allow(clippy::cast_sign_loss)]
    fn execute_branch(&mut self, bus: &impl Bus, base_addr: u32, addr_offset: i32) {
        self.reg.r[PC_INDEX] = base_addr.wrapping_add(addr_offset as _);
        self.reload_pipeline(bus);
    }

    fn meets_condition(&self, cond: u8) -> bool {
        match cond {
            // EQ
            0 => self.reg.cpsr.zero,
            // NE
            1 => !self.reg.cpsr.zero,
            // CS/HS
            2 => self.reg.cpsr.carry,
            // CC/LO
            3 => !self.reg.cpsr.carry,
            // MI
            4 => self.reg.cpsr.signed,
            // PL
            5 => !self.reg.cpsr.signed,
            // VS
            6 => self.reg.cpsr.overflow,
            // VC
            7 => !self.reg.cpsr.overflow,
            // HI
            8 => self.reg.cpsr.carry && !self.reg.cpsr.zero,
            // LS
            9 => !self.reg.cpsr.carry || self.reg.cpsr.zero,
            // GE
            10 => self.reg.cpsr.signed == self.reg.cpsr.overflow,
            // LT
            11 => self.reg.cpsr.signed != self.reg.cpsr.overflow,
            // GT
            12 => !self.reg.cpsr.zero && (self.reg.cpsr.signed == self.reg.cpsr.overflow),
            // LE
            13 => self.reg.cpsr.zero || (self.reg.cpsr.signed != self.reg.cpsr.overflow),
            // AL (Always) or Undefined in Thumb (TODO: how does it act?)
            14 => true,
            // Reserved (TODO: acts like Never in ARMv1,v2?)
            15 => false,
            _ => unreachable!(),
        }
    }

    fn execute_thumb_bl(&mut self, bus: &impl Bus, hi_part: bool, addr_offset_part: u16) {
        let addr_offset_part = u32::from(addr_offset_part);

        if hi_part {
            self.reg.r[LR_INDEX] = self.reg.r[PC_INDEX].wrapping_add(addr_offset_part << 12);
        } else {
            // Adjust for pipelining, which has us two instructions ahead.
            let return_addr = self.reg.r[PC_INDEX].wrapping_sub(self.reg.cpsr.state.instr_size());

            #[allow(clippy::cast_possible_wrap)]
            self.execute_branch(bus, self.reg.r[LR_INDEX], (addr_offset_part << 1) as _);
            self.reg.r[LR_INDEX] = return_addr | 1; // bit 0 set indicates Thumb
        }
    }

    fn execute_arm_bl(&mut self, bus: &impl Bus, addr_offset: i32) {
        // Adjust for pipelining, which has us two instructions ahead.
        self.reg.r[LR_INDEX] = self.reg.r[PC_INDEX].wrapping_sub(self.reg.cpsr.state.instr_size());
        self.execute_branch(bus, self.reg.r[PC_INDEX], addr_offset);
    }

    fn execute_mrs(&self, use_spsr: bool) -> u32 {
        // TODO: No SPSR exists in User & System mode. What happens if we attempt access?
        if use_spsr {
            self.reg.spsr.bits()
        } else {
            self.reg.cpsr.bits()
        }
    }

    fn execute_msr(&mut self, use_spsr: bool, write_flags: bool, write_control: bool, value: u32) {
        if use_spsr {
            // TODO: No SPSR exists in User & System mode. What happens if we attempt access?
            if write_control {
                // TODO: Possibly invalid mode; what's the real behaviour?
                let _ = self.reg.spsr.set_control_from_bits(value);
            }
            if write_flags {
                self.reg.spsr.set_flags_from_bits(value);
            }
        } else {
            if write_control && self.reg.cpsr.mode != OperationMode::User {
                if let Some(new_mode) = OperationMode::from_bits(value) {
                    self.reg.change_mode(new_mode);
                    self.reg.cpsr.set_control_from_bits(value).unwrap();
                } else {
                    // TODO: Invalid mode; what's the real behaviour?
                    let _ = self.reg.cpsr.set_control_from_bits(value);
                }
            }
            if write_flags {
                self.reg.cpsr.set_flags_from_bits(value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        arm7tdmi::{
            reg::{OperationState, PC_INDEX},
            Cpu,
        },
        bus::{tests::NullBus, Bus},
    };

    #[allow(clippy::struct_excessive_bools)]
    pub struct InstrTest<'a> {
        setup_fn: Option<&'a dyn Fn(&mut Cpu)>,
        state: OperationState,
        instr: u32,

        asserted_rs: [u32; 16],
        assert_signed: bool,
        assert_zero: bool,
        assert_carry: bool,
        assert_overflow: bool,
        assert_irq_disabled: bool,
        assert_fiq_disabled: bool,
    }

    impl InstrTest<'_> {
        fn new(state: OperationState, instr: u32) -> Self {
            let mut asserted_rs = [0; 16];
            asserted_rs[PC_INDEX] = 2 * state.instr_size();

            Self {
                setup_fn: None,
                state,
                instr,
                asserted_rs,
                assert_signed: false,
                assert_zero: false,
                assert_carry: false,
                assert_overflow: false,
                assert_irq_disabled: true,
                assert_fiq_disabled: true,
            }
        }

        #[must_use]
        pub fn new_arm(instr: u32) -> Self {
            Self::new(OperationState::Arm, instr)
        }

        #[must_use]
        pub fn new_thumb(instr: u16) -> Self {
            Self::new(OperationState::Thumb, instr.into())
        }
    }

    impl<'a> InstrTest<'a> {
        pub fn run_with_bus(self, bus: &mut impl Bus) -> Cpu {
            let mut cpu = Cpu::new();
            cpu.reset(bus);

            if self.state == OperationState::Thumb {
                cpu.execute_bx(bus, 1); // Enter Thumb mode.
            }
            if let Some(setup_fn) = self.setup_fn {
                setup_fn(&mut cpu);
            }

            match self.state {
                OperationState::Thumb => cpu.execute_thumb(bus, self.instr.try_into().unwrap()),
                OperationState::Arm => cpu.execute_arm(bus, self.instr),
            }

            assert_eq!(cpu.reg.r.0, self.asserted_rs);
            assert_eq!(cpu.reg.cpsr.signed, self.assert_signed, "signed flag");
            assert_eq!(cpu.reg.cpsr.zero, self.assert_zero, "zero flag");
            assert_eq!(cpu.reg.cpsr.carry, self.assert_carry, "carry flag");
            assert_eq!(cpu.reg.cpsr.overflow, self.assert_overflow, "overflow flag");
            assert_eq!(
                cpu.reg.cpsr.irq_disabled, self.assert_irq_disabled,
                "irq_disabled flag"
            );
            assert_eq!(
                cpu.reg.cpsr.fiq_disabled, self.assert_fiq_disabled,
                "fiq_disabled flag"
            );

            cpu
        }

        pub fn run(self) -> Cpu {
            self.run_with_bus(&mut NullBus)
        }

        #[must_use]
        pub fn setup(mut self, setup_fn: &'a dyn Fn(&mut Cpu)) -> Self {
            self.setup_fn = Some(setup_fn);

            self
        }

        #[must_use]
        pub fn assert_r(mut self, index: usize, r: u32) -> Self {
            self.asserted_rs[index] = r;

            self
        }

        #[must_use]
        pub fn assert_signed(mut self) -> Self {
            self.assert_signed = true;

            self
        }

        #[must_use]
        pub fn assert_zero(mut self) -> Self {
            self.assert_zero = true;

            self
        }

        #[must_use]
        pub fn assert_carry(mut self) -> Self {
            self.assert_carry = true;

            self
        }

        #[must_use]
        pub fn assert_overflow(mut self) -> Self {
            self.assert_overflow = true;

            self
        }

        #[must_use]
        pub fn assert_irq_enabled(mut self) -> Self {
            self.assert_irq_disabled = false;

            self
        }

        #[must_use]
        pub fn assert_fiq_enabled(mut self) -> Self {
            self.assert_fiq_disabled = false;

            self
        }
    }
}