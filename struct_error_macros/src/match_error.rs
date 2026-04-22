//! Implementation of the `match_error!` function-like macro.
//!
//! Zero-argument blind matching — pattern-match on errors by type name alone,
//! eliminating manual `Result` and `Unt` nesting.
//!
//! # Core behaviour
//!
//! 1. Extract all error match arms (everything except `Ok(...)`).
//! 2. Expand any united error arms into their constituent members.
//! 3. Blind-sort the error type paths.
//! 4. Generate the corresponding `Unt` nested pattern for each arm based on its sort index.
//! 5. Let the compiler perform exhaustiveness checking.

use proc_macro::TokenStream;
use quote::quote;

/// Parses `match_error!` input, supporting two syntaxes:
/// - Legacy: `match expr { ... }`
/// - Modern (recommended): `expr { ... }` (omits the `match` keyword)
fn parse_match_expr(input: proc_macro2::TokenStream) -> syn::Result<syn::ExprMatch> {
    // 先尝试旧语法：完整 match 表达式
    if let Ok(m) = syn::parse2::<syn::ExprMatch>(input.clone()) {
        return Ok(m);
    }

    // 新语法：expr { arms... }
    // 将 expr 与 { arms } 拆分，重构成 `match expr { arms }` 后再解析
    let mut expr_tokens = Vec::new();
    let mut brace_group = None;
    for tt in input {
        if brace_group.is_some() {
            continue;
        }
        match &tt {
            proc_macro2::TokenTree::Group(g) if g.delimiter() == proc_macro2::Delimiter::Brace => {
                brace_group = Some(g.clone());
            }
            _ => expr_tokens.push(tt),
        }
    }

    let brace = brace_group.ok_or_else(|| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            "match_error: expected `{ ... }` block after expression",
        )
    })?;
    let expr: proc_macro2::TokenStream = expr_tokens.into_iter().collect();

    let reconstructed = quote! {
        match #expr #brace
    };
    syn::parse2(reconstructed)
}

/// match_error! function-like proc macro 入口。
///
/// 支持两种语法：
/// - 旧语法：`match_error!(match <expr> { ... })`
/// - 新语法（推荐）：`match_error!(<expr> { ... })`
pub(crate) fn match_error(input: TokenStream) -> TokenStream {
    let input_ts = proc_macro2::TokenStream::from(input);

    // 尝试解析为 match 表达式（旧语法或新语法）
    let match_expr = match parse_match_expr(input_ts) {
        Ok(m) => m,
        Err(e) => return e.to_compile_error().into(),
    };

    let expr = &match_expr.expr;
    let arms = &match_expr.arms;

    // 分类 match arms
    let mut ok_arms: Vec<&syn::Arm> = Vec::new();
    let mut error_arms: Vec<&syn::Arm> = Vec::new();

    for arm in arms {
        match classify_pat(&arm.pat) {
            ArmType::Ok => ok_arms.push(arm),
            ArmType::Error => error_arms.push(arm),
            ArmType::CatchAll => {
                return syn::Error::new_spanned(
                    &arm.pat,
                    "match_error: catch-all patterns (`_` or bare bindings) are not allowed; \
                     all error types must be matched explicitly",
                )
                .to_compile_error()
                .into();
            }
            ArmType::Unknown => {
                return syn::Error::new_spanned(&arm.pat, "match_error: unsupported pattern type")
                    .to_compile_error()
                    .into();
            }
        }
    }

    // 收集显式错误路径（用于检测联合类型展开冲突）
    let mut explicit_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
    for arm in &error_arms {
        if let Some(path) = extract_error_path(&arm.pat) {
            explicit_paths.insert(crate::sort::path_to_string(&path));
        }
    }

    // 展开联合类型并收集所有错误 arm 数据
    let mut error_paths: Vec<syn::Path> = Vec::new();
    let mut error_arm_data: Vec<ErrorArmData> = Vec::new();

    for arm in &error_arms {
        if let Some(path) = extract_error_path(&arm.pat) {
            if crate::registry::is_united_error(&path) {
                // 禁止捕获：联合类型匹配只能是 bare path
                if !is_bare_path(&arm.pat) {
                    return syn::Error::new_spanned(
                        &arm.pat,
                        "match_error: united error matching does not support capture; \
                         use a bare path like `AppError => ...`",
                    )
                    .to_compile_error()
                    .into();
                }

                // 展开联合类型为成员 arms
                if let Some(members) = crate::registry::get_united_members(&path) {
                    for member in members {
                        if explicit_paths.contains(&member) {
                            // 显式 arm 优先，跳过该成员的展开
                            continue;
                        }
                        let member_path: syn::Path = match syn::parse_str(&member) {
                            Ok(p) => p,
                            Err(_) => {
                                return syn::Error::new_spanned(
                                    &arm.pat,
                                    format!(
                                        "match_error: invalid member path `{}` in united error",
                                        member
                                    ),
                                )
                                .to_compile_error()
                                .into();
                            }
                        };
                        error_paths.push(member_path.clone());
                        error_arm_data.push(ErrorArmData {
                            path: member_path.clone(),
                            pat: make_bare_pat(&member_path),
                            guard: arm.guard.clone(),
                            body: arm.body.clone(),
                        });
                    }
                }
            } else {
                error_paths.push(path.clone());
                error_arm_data.push(ErrorArmData {
                    path,
                    pat: arm.pat.clone(),
                    guard: arm.guard.clone(),
                    body: arm.body.clone(),
                });
            }
        }
    }

    // 盲排序
    let sorted_paths = crate::sort::sort_paths(error_paths.clone());
    let unique_paths = crate::sort::dedup_paths(sorted_paths);

    // 构建路径 -> 索引的映射
    let path_index_map: std::collections::HashMap<String, usize> = unique_paths
        .iter()
        .enumerate()
        .map(|(i, p)| (crate::sort::path_to_string(p), i))
        .collect();

    // 生成新的 match arms
    let mut new_arms: Vec<proc_macro2::TokenStream> = Vec::new();

    // Ok arms：保持不变
    for arm in &ok_arms {
        let pat = &arm.pat;
        let guard_ts = guard_to_tokens(&arm.guard);
        let body = &arm.body;
        new_arms.push(quote! {
            #pat #guard_ts => #body,
        });
    }

    // 错误 arms：根据排序索引生成 Unt 嵌套
    for data in &error_arm_data {
        let path_key = crate::sort::path_to_string(&data.path);
        if let Some(&index) = path_index_map.get(&path_key) {
            // 剥离用户手写的 Err(...) 外层，避免生成双重 Err 包装
            let stripped_pat = strip_err_wrapper(&data.pat);
            let new_pat = crate::sort::build_unt_pattern(index, unique_paths.len(), &stripped_pat);
            let guard_ts = guard_to_tokens(&data.guard);
            let body = &data.body;
            new_arms.push(quote! {
                #new_pat #guard_ts => #body,
            });
        }
    }

    let expanded = quote! {
        match #expr {
            #(#new_arms)*
        }
    };

    expanded.into()
}

/// match arm 的分类类型
enum ArmType {
    /// Ok(...) arm
    Ok,
    /// 错误类型匹配臂
    Error,
    /// 通配符或变量绑定（catch-all）
    CatchAll,
    /// 无法识别的模式
    Unknown,
}

/// 存储错误 match arm 的数据
struct ErrorArmData {
    /// 错误类型的路径
    path: syn::Path,
    /// 原始模式（保留字段绑定）
    pat: syn::Pat,
    /// guard（if 条件）
    guard: Option<(syn::token::If, Box<syn::Expr>)>,
    /// arm body
    body: Box<syn::Expr>,
}

/// 对 match arm 的 pattern 进行分类
fn classify_pat(pat: &syn::Pat) -> ArmType {
    match pat {
        // Ok(...) arm
        syn::Pat::TupleStruct(ts) => {
            if is_path_ok(&ts.path) {
                ArmType::Ok
            } else if is_path_err(&ts.path) {
                // Err(...)：剥离外层，内部可能是错误类型
                ArmType::Error
            } else {
                // 检查是否是 PascalCase 的错误类型
                if is_error_path(&ts.path) {
                    ArmType::Error
                } else {
                    ArmType::Unknown
                }
            }
        }
        syn::Pat::Path(pp) => {
            if is_path_ok(&pp.path) {
                ArmType::Ok
            } else if is_path_err(&pp.path) || is_error_path(&pp.path) {
                ArmType::Error
            } else {
                ArmType::Unknown
            }
        }
        // 通配符 _
        syn::Pat::Wild(_) => ArmType::CatchAll,
        // 裸变量绑定：PascalCase -> 错误类型，否则 -> catch-all
        syn::Pat::Ident(pi) => {
            if crate::sort::is_pascal_case(&pi.ident) {
                ArmType::Error
            } else {
                ArmType::CatchAll
            }
        }
        // 结构体模式：NotFound { id }
        syn::Pat::Struct(ps) => {
            if is_error_path(&ps.path) {
                ArmType::Error
            } else {
                ArmType::Unknown
            }
        }
        // 其他模式不支持
        _ => ArmType::Unknown,
    }
}

/// 检查模式是否是 bare path（无捕获）。
/// 用于联合类型匹配：只允许 `AppError` 或 `AppError`（Pat::Ident），
/// 不允许 `AppError(e)`、`AppError { .. }` 等。
fn is_bare_path(pat: &syn::Pat) -> bool {
    match pat {
        syn::Pat::Path(_) => true,
        syn::Pat::Ident(pi) if crate::sort::is_pascal_case(&pi.ident) => true,
        _ => false,
    }
}

/// 从路径生成一个 bare path pattern。
fn make_bare_pat(path: &syn::Path) -> syn::Pat {
    syn::Pat::Path(syn::PatPath {
        attrs: Vec::new(),
        qself: None,
        path: path.clone(),
    })
}

/// 剥离模式外层的 Err(...) 包装。若不存在则返回原模式。
fn strip_err_wrapper(pat: &syn::Pat) -> syn::Pat {
    match pat {
        syn::Pat::TupleStruct(ts) if is_path_err(&ts.path) => {
            if let Some(inner) = ts.elems.first() {
                inner.clone()
            } else {
                pat.clone()
            }
        }
        _ => pat.clone(),
    }
}

/// 从错误 pattern 中提取类型路径
fn extract_error_path(pat: &syn::Pat) -> Option<syn::Path> {
    match pat {
        syn::Pat::TupleStruct(ts) => {
            if is_path_err(&ts.path) {
                // Err(Inner) -> 提取 Inner
                if let Some(inner) = ts.elems.first() {
                    return extract_error_path(inner);
                }
            }
            Some(ts.path.clone())
        }
        syn::Pat::Path(pp) => Some(pp.path.clone()),
        syn::Pat::Ident(pi) => {
            // PascalCase 的裸标识符视为单元结构体路径
            if crate::sort::is_pascal_case(&pi.ident) {
                Some(syn::Path::from(pi.ident.clone()))
            } else {
                None
            }
        }
        syn::Pat::Struct(ps) => Some(ps.path.clone()),
        _ => None,
    }
}

/// 检查路径是否是 Ok
fn is_path_ok(path: &syn::Path) -> bool {
    path.segments.len() == 1
        && path.segments[0].ident == "Ok"
        && matches!(path.segments[0].arguments, syn::PathArguments::None)
}

/// 检查路径是否是 Err
fn is_path_err(path: &syn::Path) -> bool {
    path.segments.len() == 1
        && path.segments[0].ident == "Err"
        && matches!(path.segments[0].arguments, syn::PathArguments::None)
}

/// 将 guard 转换为 TokenStream。
fn guard_to_tokens(guard: &Option<(syn::token::If, Box<syn::Expr>)>) -> proc_macro2::TokenStream {
    match guard {
        Some((_, expr)) => quote! { if #expr },
        None => quote! {},
    }
}

/// 检查路径是否是错误类型（PascalCase 命名）。
/// 这里我们假设所有 PascalCase 的单段路径都是潜在的错误类型。
fn is_error_path(path: &syn::Path) -> bool {
    if path.segments.len() == 1 {
        let seg = &path.segments[0];
        if matches!(seg.arguments, syn::PathArguments::None) {
            return crate::sort::is_pascal_case(&seg.ident);
        }
    }
    // 多段路径（如 db::Timeout）也视为错误类型
    if let Some(last) = path.segments.last() {
        return crate::sort::is_pascal_case(&last.ident);
    }
    false
}
