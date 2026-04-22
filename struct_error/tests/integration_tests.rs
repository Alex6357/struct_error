use std::error::Error;
use struct_error::{error, match_error, throw, throws, united_error};

// ============================================================================
// #[error] tests
// ============================================================================

#[error]
pub struct PlainError;

#[error("code: {}", self.code)]
pub struct FormattedError {
    pub code: u32,
}

#[error("wrapped I/O")]
pub struct WrappedIo {
    #[error_source]
    pub source: std::io::Error,
}

#[test]
fn test_error_display_plain() {
    let err = PlainError;
    assert_eq!(err.to_string(), "PlainError");
}

#[test]
fn test_error_display_formatted() {
    let err = FormattedError { code: 42 };
    assert_eq!(err.to_string(), "code: 42");
}

#[test]
fn test_error_source() {
    let inner = std::io::Error::new(std::io::ErrorKind::NotFound, "file gone");
    let err = WrappedIo { source: inner };
    assert!(err.source().is_some());
}

// ============================================================================
// #[united_error] tests
// ============================================================================

#[united_error(PlainError, FormattedError)]
pub struct MyUnited;

#[test]
fn test_united_error_is_zst() {
    assert_eq!(std::mem::size_of::<MyUnited>(), 0);
}

// ============================================================================
// #[throws] + throw! tests
// ============================================================================

#[error]
pub struct FailA;

#[error]
pub struct FailB;

#[throws(FailA, FailB)]
pub fn may_fail_a(should_fail: bool) -> i32 {
    if should_fail {
        throw!(FailA);
    }
    42
}

#[throws(FailA, FailB)]
pub fn may_fail_b(should_fail: bool) -> i32 {
    if should_fail {
        throw!(FailB);
    }
    24
}

#[test]
fn test_throws_ok_path() {
    assert_eq!(may_fail_a(false).unwrap(), 42);
    assert_eq!(may_fail_b(false).unwrap(), 24);
}

#[test]
fn test_throws_err_path() {
    assert!(may_fail_a(true).is_err());
    assert!(may_fail_b(true).is_err());
}

#[test]
fn test_throws_implicit_ok_wrapping() {
    #[throws(FailA)]
    fn returns_value() -> i32 {
        99
    }
    assert_eq!(returns_value().unwrap(), 99);
}

#[test]
fn test_throws_unit_return() {
    #[throws(FailA)]
    fn unit_fn(throw_it: bool) {
        if throw_it {
            throw!(FailA);
        }
    }
    assert!(unit_fn(true).is_err());
    assert!(unit_fn(false).is_ok());
}

// ============================================================================
// match_error! tests
// ============================================================================

#[test]
fn test_match_error_by_type() {
    let r = may_fail_a(true);
    let got = match_error!(r {
        Ok(v) => format!("ok-{}", v),
        FailA => "got-a".to_string(),
        FailB => "got-b".to_string(),
    });
    assert_eq!(got, "got-a");
}

// ============================================================================
// match_error! united error tests
// ============================================================================

#[error]
pub struct UnitedA;

#[error]
pub struct UnitedB;

#[united_error(UnitedA, UnitedB)]
pub struct MyUnitedErr;

#[throws(MyUnitedErr)]
pub fn united_may_fail(which: u8) -> i32 {
    match which {
        1 => throw!(UnitedA),
        2 => throw!(UnitedB),
        _ => 42,
    }
}

#[test]
fn test_match_error_united_type() {
    let r_a = united_may_fail(1);
    let got_a = match_error!(r_a {
        Ok(v) => format!("ok-{}", v),
        MyUnitedErr => "united".to_string(),
    });
    assert_eq!(got_a, "united");

    let r_b = united_may_fail(2);
    let got_b = match_error!(r_b {
        Ok(v) => format!("ok-{}", v),
        MyUnitedErr => "united".to_string(),
    });
    assert_eq!(got_b, "united");
}

#[test]
fn test_match_error_united_and_explicit() {
    let r_a = united_may_fail(1);
    let got_a = match_error!(r_a {
        Ok(v) => format!("ok-{}", v),
        UnitedA => "explicit-a".to_string(),
        MyUnitedErr => "united".to_string(),
    });
    assert_eq!(got_a, "explicit-a");

    let r_b = united_may_fail(2);
    let got_b = match_error!(r_b {
        Ok(v) => format!("ok-{}", v),
        UnitedA => "explicit-a".to_string(),
        MyUnitedErr => "united".to_string(),
    });
    assert_eq!(got_b, "united");
}

#[test]
fn test_match_error_united_exhaustive() {
    let r = united_may_fail(0);
    let got = match_error!(r {
        Ok(v) => v,
        MyUnitedErr => 0,
    });
    assert_eq!(got, 42);
}

// ============================================================================
// ? propagation tests
// ============================================================================

#[throws(FailA, FailB)]
pub fn propagate_a(should_fail: bool) -> i32 {
    let v = may_fail_a(should_fail)?;
    v + 1
}

#[throws(FailA, FailB)]
pub fn propagate_b(should_fail: bool) -> i32 {
    let v = may_fail_b(should_fail)?;
    v + 1
}

#[test]
fn test_propagation_ok() {
    assert_eq!(propagate_a(false).unwrap(), 43);
    assert_eq!(propagate_b(false).unwrap(), 25);
}

#[test]
fn test_propagation_err() {
    assert!(propagate_a(true).is_err());
    assert!(propagate_b(true).is_err());
}

// ============================================================================
// Nested #[throws] (subset superset) tests
// ============================================================================

#[error]
pub struct OuterErr;

#[error]
pub struct InnerErr;

#[throws(InnerErr)]
pub fn inner(should_fail: bool) -> i32 {
    if should_fail {
        throw!(InnerErr);
    }
    1
}

#[throws(OuterErr, InnerErr)]
pub fn outer(should_fail_inner: bool) -> i32 {
    let v = inner(should_fail_inner)?;
    v + 10
}

#[test]
fn test_nested_throws_ok() {
    assert_eq!(outer(false).unwrap(), 11);
}

#[test]
fn test_nested_throws_err() {
    assert!(outer(true).is_err());
}
