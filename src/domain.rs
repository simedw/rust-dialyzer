//! The semantic universe is fixed by the Cargo package, never by process state.

#[cfg(otp_domain = "28")]
pub const MAJOR: &str = "28";
#[cfg(otp_domain = "29")]
pub const MAJOR: &str = "29";

#[cfg(otp_domain = "28")]
pub const UNION_ARITY: usize = 8;
#[cfg(otp_domain = "29")]
pub const UNION_ARITY: usize = 9;

pub fn check_major(actual: &str) -> Result<(), String> {
    if actual == MAJOR {
        Ok(())
    } else {
        Err(format!("OTP {actual} input cannot be analyzed by the OTP {MAJOR} Rust backend"))
    }
}
