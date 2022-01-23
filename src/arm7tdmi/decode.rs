use super::Cpu;

impl Cpu {
    fn decode_thumb(op: u16) {
        let hi8 = (op >> 8) as u8;
        let hi6 = hi8 >> 2;
        let hi5 = hi8 >> 3;
        let hi4 = hi8 >> 4;
        let hi3 = hi8 >> 5;
        let bit9 = hi8 & 0b10 != 0;

        match (hi3, hi4, hi5, hi6, hi8, bit9) {
            (_, _, _, _, 0b1011_0000, _) => todo!("format 13"),
            (_, _, _, _, 0b1011_1111, _) => todo!("format 17"),
            (_, _, _, 0b01_0000, _, _) => todo!("format 4"),
            (_, _, _, 0b01_0001, _, _) => todo!("format 5"),
            (_, _, 0b0_0011, _, _, _) => todo!("format 2"),
            (_, _, 0b0_1001, _, _, _) => todo!("format 6"),
            (_, _, 0b1_1100, _, _, _) => todo!("format 18"),
            (_, 0b0101, _, _, _, true) => todo!("format 8"),
            (_, 0b0101, _, _, _, false) => todo!("format 7"),
            (_, 0b1000, _, _, _, _) => todo!("format 10"),
            (_, 0b1001, _, _, _, _) => todo!("format 11"),
            (_, 0b1010, _, _, _, _) => todo!("format 12"),
            (_, 0b1011, _, _, _, _) => todo!("format 14"),
            (_, 0b1100, _, _, _, _) => todo!("format 15"),
            (_, 0b1101, _, _, _, _) => todo!("format 16"),
            (_, 0b1111, _, _, _, _) => todo!("format 19"),
            (0b000, _, _, _, _, _) => todo!("format 1"),
            (0b001, _, _, _, _, _) => todo!("format 3"),
            (0b011, _, _, _, _, _) => todo!("format 9"),
            _ => todo!("undefined instruction"),
        }
    }
}
