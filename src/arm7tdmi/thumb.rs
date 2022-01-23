use crate::arm7tdmi::{reg::NamedGeneralRegister::Pc, Exception};

use super::{reg::Registers, Cpu};

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
    Undefined,
}

#[must_use]
fn decode_format(instr: u16) -> InstructionFormat {
    use InstructionFormat::*;

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
    pub(crate) fn pc_thumb_addr(&self) -> u32 {
        self.r[Pc] & !1
    }
}

impl Cpu {
    pub(crate) fn execute_thumb_instr(&mut self, instr: u16) {
        use InstructionFormat::*;

        // TODO: add to CPU cycle counts when implemented
        match decode_format(instr) {
            MoveShiftedReg => todo!(),
            AddSub => todo!(),
            MoveCmpAddSubImm => todo!(),
            AluOp => todo!(),
            HiRegOpBranchExchange => todo!(),
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
