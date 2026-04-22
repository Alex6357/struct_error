#![no_std]

//! Modern, flat, zero-cost error flow based on pure struct errors.
//!
//! # ⚠️ **WARNING: UNAUDITED CODE**
//!
//! This crate was AI-generated and has not undergone human review. It likely contains bugs and should **NOT** be used in production. The API is subject to change after audit.
//!
//! `struct_error` inverts the traditional Rust error model:
//! - **Errors are first-class structs**, not enum variants.
//! - **No manual `Ok`/`Err` wrapping** inside `#[throws]` functions.
//! - **Pattern match by type name** without destructuring nested `Result`/`Enum` layers.
//! - **Zero runtime cost**: everything is resolved at compile time via procedural macros.
//!
//! # Core Concepts
//!
//! | Item | Purpose |
//! |------|---------|
//! | [`#[error]`](macro@error) | Define an atomic error struct with auto-derived `Debug`, `Display`, and `Error`. |
//! | [`#[united_error]`](macro@united_error) | Create a compile-time alias for a set of errors. |
//! | [`#[throws]`](macro@throws) | Rewrite a function to implicitly return `Result<T, Unt<...>>` and intercept `?`. |
//! | [`match_error!`](macro@match_error) | Blind, type-driven pattern matching on errors. |
//! | [`throw!`] | Explicitly throw an error inside a `#[throws]` function. |
//! | [`Unt`] | Runtime heterogeneous list (nested enum) representing the implicit union. |
//! | [`End`] | Uninhabited terminator for the `Unt` HList. |
//!
//! # Examples
//!
//! A complete end-to-end example:
//!
//! ```
//! use struct_error::{error, united_error, throws, match_error, throw};
//!
//! #[error("resource not found: {}", self.id)]
//! pub struct NotFound {
//!     pub id: u64,
//! }
//!
//! #[error("connection timed out after {}ms", self.ms)]
//! pub struct Timeout {
//!     pub ms: u64,
//! }
//!
//! #[united_error(NotFound, Timeout)]
//! pub struct AppError;
//!
//! #[throws(NotFound, Timeout)]
//! pub fn fetch_resource(id: u64) -> String {
//!     if id == 0 {
//!         throw!(NotFound { id });
//!     }
//!     if id > 100 {
//!         throw!(Timeout { ms: 5000 });
//!     }
//!     format!("resource-{}", id)
//! }
//!
//! #[throws(NotFound, Timeout)]
//! pub fn process(id: u64) -> String {
//!     let res = fetch_resource(id)?;
//!     res.to_uppercase()
//! }
//!
//! fn main() {
//!     let result = process(0);
//!     match_error!(result {
//!         Ok(v) => println!("success: {}", v),
//!         NotFound { id } => println!("not found: {}", id),
//!         Timeout { ms } => println!("timeout: {}ms", ms),
//!     });
//! }
//! ```

/// Heterogeneous list (nested enum) — runtime representation of the implicit error union.
///
/// For error types `A`, `B`, `C`, the return type of a `#[throws]` function becomes:
///
/// ```ignore
/// Result<T, Unt<A, Unt<B, Unt<C, End>>>>
/// ```
///
/// `Unt` is rarely manipulated directly; `#[throws]` and `match_error!` handle the nesting
/// automatically via a blind sorting algorithm that guarantees consistent ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unt<H, T> {
    Here(H),
    There(T),
}

/// Uninhabited terminator for the `Unt` HList.
///
/// Using an empty enum (rather than a unit struct) makes `There(End)` unconstructible
/// at the type level, allowing the compiler's exhaustiveness checker to work correctly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {}

/// Explicitly throw an error inside a `#[throws]` function.
///
/// Replaces the semantics of `?` and `return Err(...)`. Inside a function annotated with
/// `#[throws]`, `throw!` uses the locally injected `__StructErrorInto` trait to box the
/// error into the correct `Unt` position.
///
/// # Examples
///
/// ```ignore
/// #[throws(NotFound)]
/// fn risky() {
///     throw!(NotFound { id: 42 });
/// }
/// ```
#[macro_export]
macro_rules! throw {
    ($err:expr) => {
        return ::core::result::Result::Err(__StructErrorInto::into_unt($err))
    };
}

// 重新导出过程宏，用户只需 `use struct_error::{error, united_error, throws, match_error, throw};`
pub use struct_error_macros::*;

// 公开 macro_magic，确保 proc macro 生成的路径可解析
pub extern crate macro_magic;
