# struct_error

Modern, flat, zero-cost error flow based on pure struct errors.

## ⚠️ **WARNING: UNAUDITED CODE**

This crate was AI-generated and has not undergone human review. It likely contains bugs and should **NOT** be used in production. The API is subject to change after audit.

## Philosophy

Traditional Rust error handling forces you to wrap everything in `Result` and nest errors inside `enum` variants. `struct_error` inverts this model:

- **Errors are first-class structs**, not enum variants.
- **No manual `Ok`/`Err` wrapping** inside `#[throws]` functions.
- **Pattern match by type name** without destructuring nested `Result`/`Enum` layers.
- **Zero runtime cost**: everything is resolved at compile time via procedural macros.

## Features

- `#[error]` — Define atomic error structs with auto-derived `Debug`, `Display`, and `Error`.
- `#[united_error]` — Create compile-time aliases for error sets.
- `#[throws]` — Implicitly return `Result<T, Unt<...>>` and rewrite control flow.
- `match_error!` — Blind, type-driven pattern matching on errors.
- `throw!` — Explicitly throw an error inside a `#[throws]` function.

## Quick Start

Add `struct_error` to your `Cargo.toml`:

```toml
[dependencies]
struct_error = "0.0.1"
```

### Define Errors

```rust
use struct_error::error;

#[error("resource not found: {}", id)]
pub struct NotFound {
    pub id: u64,
}

#[error("connection timed out after {}ms", ms)]
pub struct Timeout {
    pub ms: u64,
}
```

### Combine Errors

```rust
use struct_error::united_error;

#[united_error(NotFound, Timeout)]
pub struct AppError;
```

### Throw and Propagate

```rust
use struct_error::{throws, throw};

#[throws(NotFound, Timeout)]
pub fn fetch_resource(id: u64) -> String {
    if id == 0 {
        throw!(NotFound { id });
    }
    if id == 99 {
        throw!(Timeout { ms: 5000 });
    }
    format!("resource-{}", id)
}

#[throws(NotFound, Timeout)]
pub fn process(id: u64) -> String {
    let res = fetch_resource(id)?; // ? propagates automatically
    res.to_uppercase()
}
```

### Match Errors

```rust
use struct_error::match_error;

fn main() {
    let result = process(0);

    match_error!(result {
        Ok(v) => println!("success: {}", v),
        NotFound { id } => println!("not found: {}", id),
        Timeout { ms } => println!("timeout: {}ms", ms),
    });
}
```

## How It Works

### Blind Sorting

`#[throws]` and `match_error!` must agree on the exact nesting order of the implicit `Unt` HList. This is achieved by a **blind sorting** algorithm that sorts error type paths lexicographically by their tail segment first, then walks backward. If two paths share the same tail but differ in qualification (e.g. `Timeout` vs `db::Timeout`), compilation aborts with an ambiguity error. More detailed path must be provided (e.g. `net::Timeout` vs `db::Timeout`).

### Unt HList

At runtime, the error union is represented as a nested enum:

```rust
pub enum Unt<H, T> {
    Here(H),
    There(T),
}
```

For three errors `A`, `B`, `C`, the return type becomes:

```rust
Result<T, Unt<A, Unt<B, Unt<C, End>>>>
```

where `End` is an uninhabited terminator that enables exhaustiveness checking.

### Two-Phase Expansion

`#[throws]` expands in two phases:

1. **Phase 1** (`throws` proc macro): Uses `macro_magic::forward_tokens!` to collect the AST tokens of each error type.
2. **Phase 2** (`__throws_impl`): Receives the collected tokens, deduplicates and sorts the error paths, generates the `Unt` return type, injects the local `__StructErrorInto` trait, and rewrites the function body AST to intercept `?` and wrap returns in `Ok`.

## License

Licensed under either of:

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT license](LICENSE-MIT)

at your option.
