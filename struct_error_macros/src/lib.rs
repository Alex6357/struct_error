//! Procedural macro implementations for the `struct_error` crate.
//!
//! # ⚠️ **WARNING: UNAUDITED CODE**
//!
//! This crate was AI-generated and has not undergone human review. It likely contains bugs and should **NOT** be used in production. The API is subject to change after audit.

use proc_macro::TokenStream;

mod error;
mod match_error;
mod sort;
mod throws;
mod united_error;
mod utils;

/// Defines an atomic error struct, automatically deriving `Debug`, `Display`, and `Error`.
///
/// Also attaches `#[macro_magic::export_tokens]` so the struct's AST is readable by other
/// macros (notably `#[throws]` and `match_error!`).
///
/// # Syntax
///
/// ```ignore
/// #[error]
/// pub struct NotFound;
///
/// #[error("resource not found: {}", id)]
/// pub struct NotFound {
///     pub id: u64,
/// }
/// ```
///
/// A field may be annotated with `#[error_source]` to implement the `Error::source()` method.
///
/// # Examples
///
/// ```ignore
/// use struct_error::error;
///
/// #[error("not found: {}", id)]
/// pub struct NotFound {
///     pub id: u64,
/// }
///
/// #[error("IO failed")]
/// pub struct IoError {
///     #[error_source]
///     pub inner: std::io::Error,
/// }
/// ```
#[proc_macro_attribute]
pub fn error(attr: TokenStream, item: TokenStream) -> TokenStream {
    error::error(attr, item)
}

/// Defines a compile-time alias for a set of errors (a *united error*).
///
/// The generated struct is a zero-sized type (ZST) that carries no runtime data. It serves
/// purely as a named grouping of error types, allowing `#[throws]` and `match_error!` to
/// refer to the whole set at once.
///
/// # Syntax
///
/// ```ignore
/// use struct_error::united_error;
///
/// #[united_error(NotFound, Timeout)]
/// pub struct AppError;
/// ```
///
/// # Examples
///
/// ```ignore
/// use struct_error::{error, united_error, throws, match_error, throw};
///
/// #[error]
/// pub struct NotFound;
///
/// #[error]
/// pub struct Timeout;
///
/// #[united_error(NotFound, Timeout)]
/// pub struct AppError;
///
/// #[throws(AppError)]
/// fn risky() {
///     throw!(NotFound);
/// }
/// ```
#[proc_macro_attribute]
pub fn united_error(attr: TokenStream, item: TokenStream) -> TokenStream {
    united_error::united_error(attr, item)
}

/// Replaces the function's return type with an implicit error union and rewrites control flow.
///
/// `#[throws]` performs a two-phase expansion:
/// 1. **Phase 1** — collects AST tokens from each listed error type via `macro_magic::forward_tokens!`.
/// 2. **Phase 2** — deduplicates and blind-sorts the error paths, generates the `Unt` HList
///    return type, injects a local `__StructErrorInto` trait, and rewrites the function body
///    to intercept `?` operators and wrap successful returns in `Ok(...)`.
///
/// # Syntax
///
/// ```ignore
/// #[throws(ErrorA, ErrorB, ErrorC)]
/// pub fn foo() -> T { ... }
/// ```
///
/// The original return type `T` is preserved; the function implicitly returns
/// `Result<T, Unt<ErrorA, Unt<ErrorB, Unt<ErrorC, End>>>>`.
///
/// # Examples
///
/// ```ignore
/// use struct_error::{error, throws, throw};
///
/// #[error]
/// pub struct NotFound;
///
/// #[throws(NotFound)]
/// pub fn fetch(id: u64) -> String {
///     if id == 0 {
///         throw!(NotFound);
///     }
///     format!("resource-{}", id)
/// }
/// ```
#[proc_macro_attribute]
pub fn throws(attr: TokenStream, item: TokenStream) -> TokenStream {
    throws::throws(attr, item)
}

/// Blind, type-driven pattern matching on errors.
///
/// Eliminates manual `Result` and `Unt` nesting by matching error types directly.
/// Error arms are sorted automatically using the same blind sorting algorithm as `#[throws]`,
/// guaranteeing that declaration sites and match sites agree on the `Unt` nesting order.
///
/// Catch-all patterns (`_` or bare bindings) are **not allowed**; the compiler enforces
/// exhaustiveness through the uninhabited `End` terminator.
///
/// # Syntax
///
/// ```ignore
/// match_error!(expr {
///     Ok(v) => ...,
///     NotFound => ...,
///     Timeout { ms } => ...,
/// })
/// ```
///
/// The `Ok(...)` arm is preserved as-is. Error arms may use unit structs, struct
/// destructuring, or tuple-struct patterns. All error types in the implicit union must be
/// matched explicitly.
///
/// # Examples
///
/// ```ignore
/// use struct_error::{error, throws, match_error, throw};
///
/// #[error]
/// pub struct NotFound;
///
/// #[error]
/// pub struct Timeout;
///
/// #[throws(NotFound, Timeout)]
/// fn risky() -> i32 {
///     throw!(NotFound);
///     42
/// }
///
/// fn main() {
///     let r = risky();
///     match_error!(r {
///         Ok(v) => println!("{}", v),
///         NotFound => println!("not found"),
///         Timeout => println!("timeout"),
///     });
/// }
/// ```
#[proc_macro]
pub fn match_error(input: TokenStream) -> TokenStream {
    match_error::match_error(input)
}

/// Internal attribute macro used as the CPS callback for `macro_magic::forward_tokens!`.
///
/// Receives a forwarded item together with accumulated state (remaining paths, accumulator,
/// function metadata), chains the next `forward_tokens!` call if paths remain, or calls
/// `__throws_impl` when all tokens have been collected.
///
/// This macro is **not** intended for direct use.
#[proc_macro_attribute]
pub fn __throws_cps(attr: TokenStream, item: TokenStream) -> TokenStream {
    throws::__throws_cps(attr, item)
}

/// Internal function-like proc macro called by `__throws_cps` at the end of the token
/// forwarding chain.
///
/// Receives all collected forwarded tokens plus the original function signature and body,
/// then performs the actual signature rewriting and AST transformation.
///
/// This macro is **not** intended for direct use.
#[proc_macro]
pub fn __throws_impl(input: TokenStream) -> TokenStream {
    throws::__throws_impl(input)
}

/// Internal attribute macro that encodes the member list on structs generated by
/// `#[united_error]`.
///
/// The compiler ignores this attribute; it is only read by `__throws_impl` when parsing
/// forwarded tokens to expand a united error into its constituent types.
///
/// This macro is **not** intended for direct use.
#[proc_macro_attribute]
pub fn __struct_error_members(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}
