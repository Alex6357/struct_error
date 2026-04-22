//! Procedural macro implementations for the `struct_error` crate.
//!
//! # ⚠️ **WARNING: UNAUDITED CODE**
//!
//! This crate was AI-generated and has not undergone human review. It likely contains bugs and should **NOT** be used in production. The API is subject to change after audit.

use proc_macro::TokenStream;

mod error;
mod match_error;
mod registry;
mod sort;
mod throws;
mod united_error;
mod utils;

/// Defines an atomic error struct, automatically deriving `Debug`, `Display`, and `Error`.
///
/// Also registers the struct in a global registry so other macros can resolve it.
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
/// Automatically expands united errors into their constituent types. The error paths are
/// deduplicated and blind-sorted, then the `Unt` HList return type is generated together with
/// a local `__StructErrorInto` trait. The function body is rewritten to intercept `?` operators
/// and wrap successful returns in `Ok(...)`.
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
/// # United error matching
///
/// A `#[united_error]` can be matched with a bare path. The arm is automatically expanded
/// to cover every constituent error type. Capture is not allowed on united errors.
///
/// ```ignore
/// #[united_error(NotFound, Timeout)]
/// pub struct AppError;
///
/// match_error!(result {
///     Ok(v) => ...,
///     AppError => ...,  // expands to NotFound => ... and Timeout => ...
/// })
/// ```
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

/// Internal attribute macro that encodes the member list on structs generated by
/// `#[united_error]`.
///
/// The compiler ignores this attribute; it serves as a marker for external tooling.
///
/// This macro is **not** intended for direct use.
#[proc_macro_attribute]
pub fn __struct_error_members(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}
