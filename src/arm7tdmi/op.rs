use intbits::Bits;

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
            self.reg.cpsr.carry = result.bit(31);
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
        self.reg.cpsr.carry = result.bit(0);
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
        self.reg.cpsr.carry = result.bit(0);
        let result = (result >> 1) as _;
        self.reg.cpsr.set_nz_from(result);

        result
    }

    pub(super) fn execute_ror(&mut self, value: u32, offset: u8) -> u32 {
        let mut result = value;
        if offset > 0 {
            result = value.rotate_right(u32::from(offset) - 1);
            self.reg.cpsr.carry = result.bit(0);
            result = result.rotate_right(1);
        }
        self.reg.cpsr.set_nz_from(result);

        result
    }

    pub(super) fn execute_bx(&mut self, bus: &impl Bus, addr: u32) {
        self.reg.cpsr.state = if addr.bit(0) {
            OperationState::Thumb
        } else {
            OperationState::Arm
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
            if r_list.bit(0) {
                bus.write_word_aligned(self.reg.r[r_base_addr], self.reg.r[r]);
                self.reg.r[r_base_addr] = self.reg.r[r_base_addr].wrapping_add(4);
            }
            r_list >>= 1;
        }
    }

    pub(super) fn execute_ldmia(&mut self, bus: &impl Bus, r_base_addr: usize, mut r_list: u8) {
        // TODO: emulate weird invalid r_list behaviour? (empty r_list, r_list with r_base_addr)
        for r in 0..8 {
            if r_list.bit(0) {
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
            if r_list.bit(7) {
                self.reg.r[SP_INDEX] = self.reg.r[SP_INDEX].wrapping_sub(4);
                bus.write_word_aligned(self.reg.r[SP_INDEX], self.reg.r[r]);
            }
            r_list <<= 1;
        }
    }

    pub(super) fn execute_pop(&mut self, bus: &impl Bus, mut r_list: u8, pop_pc: bool) {
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
    pub(super) fn execute_branch(&mut self, bus: &impl Bus, base_addr: u32, addr_offset: i32) {
        self.reg.r[PC_INDEX] = base_addr.wrapping_add(addr_offset as _);
        self.reload_pipeline(bus);
    }

    pub(super) fn meets_condition(&self, cond: u8) -> bool {
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
            4 => self.reg.cpsr.negative,
            // PL
            5 => !self.reg.cpsr.negative,
            // VS
            6 => self.reg.cpsr.overflow,
            // VC
            7 => !self.reg.cpsr.overflow,
            // HI
            8 => self.reg.cpsr.carry && !self.reg.cpsr.zero,
            // LS
            9 => !self.reg.cpsr.carry || self.reg.cpsr.zero,
            // GE
            10 => self.reg.cpsr.negative == self.reg.cpsr.overflow,
            // LT
            11 => self.reg.cpsr.negative != self.reg.cpsr.overflow,
            // GT
            12 => !self.reg.cpsr.zero && (self.reg.cpsr.negative == self.reg.cpsr.overflow),
            // LE
            13 => self.reg.cpsr.zero || (self.reg.cpsr.negative != self.reg.cpsr.overflow),
            // AL (Always) or Undefined in Thumb (TODO: how does it act?)
            14 => true,
            // Reserved (TODO: acts like Never in ARMv1,v2?)
            15 => false,
            _ => unreachable!(),
        }
    }

    pub(super) fn execute_thumb_bl(
        &mut self,
        bus: &impl Bus,
        hi_part: bool,
        addr_offset_part: u16,
    ) {
        let addr_offset_part = u32::from(addr_offset_part);

        if hi_part {
            self.reg.r[LR_INDEX] = self.reg.r[PC_INDEX].wrapping_add(addr_offset_part << 12);
        } else {
            // Adjust for pipelining, which has us two instructions ahead.
            let return_addr = self.reg.r[PC_INDEX].wrapping_sub(self.reg.cpsr.state.instr_size());

            #[allow(clippy::cast_possible_wrap)]
            self.execute_branch(bus, self.reg.r[LR_INDEX], (addr_offset_part << 1) as _);
            self.reg.r[LR_INDEX] = return_addr | 1; // bit 0 set indicates THUMB
        }
    }

    pub(super) fn execute_arm_bl(&mut self, bus: &impl Bus, addr_offset: i32) {
        // Adjust for pipelining, which has us two instructions ahead.
        self.reg.r[LR_INDEX] = self.reg.r[PC_INDEX].wrapping_sub(self.reg.cpsr.state.instr_size());
        self.execute_branch(bus, self.reg.r[PC_INDEX], addr_offset);
    }
}

#[cfg(test)]
pub(super) mod tests {
    use crate::{
        arm7tdmi::{
            reg::{OperationState, PC_INDEX},
            Cpu,
        },
        bus::{tests::NullBus, Bus},
    };

    #[allow(clippy::struct_excessive_bools)]
    pub struct InstrTest<'a> {
        setup: &'a dyn Fn(&mut Cpu),
        state: OperationState,
        instr: u32,

        asserted_rs: [u32; 16],
        assert_negative: bool,
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
                setup: &|_| {},
                state,
                instr,
                asserted_rs,
                assert_negative: false,
                assert_zero: false,
                assert_carry: false,
                assert_overflow: false,
                assert_irq_disabled: false,
                assert_fiq_disabled: false,
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

            // Act like the CPU started with interrupts enabled.
            cpu.reg.cpsr.irq_disabled = false;
            cpu.reg.cpsr.fiq_disabled = false;

            if self.state == OperationState::Thumb {
                cpu.execute_bx(bus, 1); // Enter Thumb mode.
            }

            (self.setup)(&mut cpu);

            match self.state {
                OperationState::Thumb => cpu.execute_thumb(bus, self.instr.try_into().unwrap()),
                OperationState::Arm => cpu.execute_arm(bus, self.instr),
            }

            assert_eq!(cpu.reg.r.0, self.asserted_rs);
            assert_eq!(cpu.reg.cpsr.negative, self.assert_negative, "negative flag");
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
        pub fn setup(mut self, setup: &'a dyn Fn(&mut Cpu)) -> Self {
            self.setup = setup;
            self
        }

        #[must_use]
        pub fn assert_r(mut self, index: usize, r: u32) -> Self {
            self.asserted_rs[index] = r;
            self
        }

        #[must_use]
        pub fn assert_negative(mut self) -> Self {
            self.assert_negative = true;
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
        pub fn assert_irq_disabled(mut self) -> Self {
            self.assert_irq_disabled = true;
            self
        }

        #[must_use]
        pub fn assert_fiq_disabled(mut self) -> Self {
            self.assert_fiq_disabled = true;
            self
        }
    }
}
