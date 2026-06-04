//! Floating-point (D-form) — arithmetic, compare, GPR↔FPR move,
//! int↔fp convert.

use crate::reg::{Fpr, Gpr};

/// FADD Dd, Dn, Dm — f64 add. ARM ARM C6.2.96. Base = 0x1E60_2800.
pub fn fadd_d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x1E60_2800 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// FSUB Dd, Dn, Dm. ARM ARM C6.2.144.
pub fn fsub_d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x1E60_3800 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// FMUL Dd, Dn, Dm. ARM ARM C6.2.131.
pub fn fmul_d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x1E60_0800 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// FDIV Dd, Dn, Dm. ARM ARM C6.2.103.
pub fn fdiv_d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x1E60_1800 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// FMOV Dd, Xn — reinterpret the 64-bit value in Xn as an f64. No
/// conversion. ARM ARM C6.2.116. Base = 0x9E67_0000.
pub fn fmov_d_from_x(rd: Fpr, rn: Gpr) -> u32 {
    0x9E67_0000 | (rn.idx() << 5) | rd.idx()
}

/// FMOV Xd, Dn — reinterpret the 64-bit value in Dn as a u64.
/// ARM ARM C6.2.115. Base = 0x9E66_0000.
pub fn fmov_x_from_d(rd: Gpr, rn: Fpr) -> u32 {
    0x9E66_0000 | (rn.idx() << 5) | rd.idx()
}

/// FCMP Dn, Dm — set NZCV from `Dn vs Dm`. NaN sets the "unordered"
/// pattern (N=0,Z=0,C=1,V=1). ARM ARM C6.2.98. Base = 0x1E60_2000.
pub fn fcmp_d(rn: Fpr, rm: Fpr) -> u32 {
    0x1E60_2000 | (rm.idx() << 16) | (rn.idx() << 5)
}

/// SCVTF Dd, Xn — signed int64 → f64. ARM ARM C6.2.353. Base = 0x9E62_0000.
pub fn scvtf_d_x(rd: Fpr, rn: Gpr) -> u32 {
    0x9E62_0000 | (rn.idx() << 5) | rd.idx()
}

/// FCVTZS Xd, Dn — f64 → signed int64, round toward zero.
/// ARM ARM C6.2.114. Base = 0x9E78_0000.
pub fn fcvtzs_x_d(rd: Gpr, rn: Fpr) -> u32 {
    0x9E78_0000 | (rn.idx() << 5) | rd.idx()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fadd_d0_d1_d2_matches_arm_arm() {
        assert_eq!(fadd_d(Fpr::V0, Fpr::V1, Fpr::V2), 0x1E62_2820);
    }

    #[test]
    fn fsub_d0_d1_d2_matches_arm_arm() {
        assert_eq!(fsub_d(Fpr::V0, Fpr::V1, Fpr::V2), 0x1E62_3820);
    }

    #[test]
    fn fmul_d0_d1_d2_matches_arm_arm() {
        assert_eq!(fmul_d(Fpr::V0, Fpr::V1, Fpr::V2), 0x1E62_0820);
    }

    #[test]
    fn fdiv_d0_d1_d2_matches_arm_arm() {
        assert_eq!(fdiv_d(Fpr::V0, Fpr::V1, Fpr::V2), 0x1E62_1820);
    }

    #[test]
    fn fmov_d0_from_x9_matches_arm_arm() {
        assert_eq!(fmov_d_from_x(Fpr::V0, Gpr::X9), 0x9E67_0120);
    }

    #[test]
    fn fmov_x0_from_d9_matches_arm_arm() {
        assert_eq!(fmov_x_from_d(Gpr::X0, Fpr::V9), 0x9E66_0120);
    }

    #[test]
    fn fcmp_d16_d17_matches_arm_arm() {
        assert_eq!(fcmp_d(Fpr::V16, Fpr::V17), 0x1E71_2200);
    }

    #[test]
    fn scvtf_d0_from_x9_matches_arm_arm() {
        assert_eq!(scvtf_d_x(Fpr::V0, Gpr::X9), 0x9E62_0120);
    }

    #[test]
    fn fcvtzs_x0_from_d16_matches_arm_arm() {
        assert_eq!(fcvtzs_x_d(Gpr::X0, Fpr::V16), 0x9E78_0200);
    }
}
