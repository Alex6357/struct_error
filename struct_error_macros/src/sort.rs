//! Blind Sorting Algorithm
//!
//! To guarantee that `#[throws]` (declaration site) and `match_error!` (match site) agree on
//! the exact nesting order of the implicit `Unt` HList, all error type paths are sorted
//! lexicographically by a stable dictionary sort.
//!
//! # Core rules
//!
//! 1. **Path normalisation** — strip leading `::`.
//! 2. **Generic stripping** — `PathArguments` are removed before comparison.
//! 3. **Reverse segment comparison** — the last segment is compared first, then the algorithm
//!    walks backward toward the crate root.
//! 4. **Ambiguity trap** — if one path is a prefix of the other (e.g. `Timeout` vs
//!    `db::Timeout`), compilation aborts with a `compile_error!` message.

use syn::Path;

/// 对错误类型路径列表进行字典序稳定排序。
///
/// 排序前会先进行路径标准化与泛型剥离。
/// 返回排序后的列表（保留原始含泛型的路径）。
/// 使用稳定排序，确保去重后的顺序可预测。
pub(crate) fn sort_paths(mut paths: Vec<Path>) -> Vec<Path> {
    // 路径标准化：去除前导双冒号，消除 syn::Path::leading_colon 差异
    for path in &mut paths {
        path.leading_colon = None;
    }

    // 使用稳定排序，确保去重后的顺序可预测
    paths.sort_by(compare_paths_blind);
    paths
}

/// Blind-sort core: reverse-segment comparison.
///
/// Segments are collected as identifier strings (without generics), then compared from the
/// tail backward. This means the struct name takes precedence over the module path.
fn compare_paths_blind(a: &Path, b: &Path) -> std::cmp::Ordering {
    let a_segments = extract_segments(a);
    let b_segments = extract_segments(b);

    let max_len = a_segments.len().max(b_segments.len());

    // 倒序遍历：从最后一个段开始向前比较
    for i in 0..max_len {
        let a_idx = a_segments.len().saturating_sub(1 + i);
        let b_idx = b_segments.len().saturating_sub(1 + i);

        let a_has = i < a_segments.len();
        let b_has = i < b_segments.len();

        if a_has && b_has {
            let cmp = a_segments[a_idx].cmp(&b_segments[b_idx]);
            if cmp != std::cmp::Ordering::Equal {
                return cmp;
            }
            // 若相等，继续比较更前一级
        } else if a_has && !b_has {
            // 歧义陷阱：a 还有前缀，但 b 已经耗尽
            // 这意味着两个路径基名相同，但一个是短路径（无模块前缀），另一个有前缀
            panic_on_ambiguity(&a_segments, &b_segments);
        } else if !a_has && b_has {
            // 歧义陷阱：b 还有前缀，但 a 已经耗尽
            panic_on_ambiguity(&a_segments, &b_segments);
        }
        // 两者都已耗尽 → 继续循环（理论上不会发生，因为 max_len 已经处理）
    }

    std::cmp::Ordering::Equal
}

/// Extracts plain identifier strings from a path (without generic arguments).
fn extract_segments(path: &Path) -> Vec<String> {
    path.segments
        .iter()
        .map(|seg| seg.ident.to_string())
        .collect()
}

/// Ambiguity trap: aborts compilation via `compile_error!`.
fn panic_on_ambiguity(a: &[String], b: &[String]) {
    let a_full = a.join("::");
    let b_full = b.join("::");

    // 找出冲突的基名（末项）
    let base = a.last().unwrap_or(&String::new()).clone();

    let error_msg = format!(
        "Ambiguous error types: `{base}` may conflict with `{a_full}` and `{b_full}`. \
         Please explicitly qualify the module path for `{base}` to guarantee safe sorting."
    );

    // 生成 compile_error! TokenStream。由于这里是在排序阶段，
    // 我们通过 panic 并将错误消息嵌入 quote! 中。
    // 实际上更好的做法是在 proc macro 中返回 compile_error! TokenStream。
    // 为了简化，我们 panic 并在上层捕获。
    let tokens = quote::quote! {
        compile_error!(#error_msg);
    };

    // 将 compile_error! 作为 panic 消息抛出
    panic!("STRUCT_ERROR_AMBIGUITY:{}:{}", base, tokens);
}

/// 将排序后的路径列表去重（基于标准化后的字符串表示）。
///
/// 保留第一次出现的原始路径。
pub(crate) fn dedup_paths(paths: Vec<Path>) -> Vec<Path> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();

    for path in paths {
        let key = path_to_string(&path);
        if seen.insert(key) {
            result.push(path);
        }
    }

    result
}

/// 将路径转换为标准化字符串（用于 HashSet 去重）。
pub(crate) fn path_to_string(path: &Path) -> String {
    path.segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

/// 根据路径在排序后列表中的索引，生成 Unt 嵌套链中的构造表达式。
///
/// 例如，索引 0 对应 `Unt::Here(inner)`，索引 1 对应
/// `Unt::There(Unt::Here(inner))`，以此类推。
///
/// `inner` 是填入最内层的表达式/模式 TokenStream（如 `self`、`expr` 或 `pat`）。
pub(crate) fn build_unt_nesting(
    index: usize,
    _total: usize,
    inner: &dyn quote::ToTokens,
) -> proc_macro2::TokenStream {
    let mut result = quote::quote! { ::struct_error::Unt::Here(#inner) };
    // 在 Here 外层包裹 index 层 There
    for _ in 0..index {
        result = quote::quote! { ::struct_error::Unt::There(#result) };
    }
    result
}

/// 根据路径在排序后列表中的索引，生成 Unt 模式匹配的嵌套结构。
///
/// 用于 `#[match_error]` 生成 match arm 的左侧模式。
pub(crate) fn build_unt_pattern(
    index: usize,
    _total: usize,
    pat: &syn::Pat,
) -> proc_macro2::TokenStream {
    let mut result = quote::quote! { ::struct_error::Unt::Here(#pat) };
    // 在 Here 外层包裹 index 层 There
    for _ in 0..index {
        result = quote::quote! { ::struct_error::Unt::There(#result) };
    }
    // 最终包裹 Err(...)
    quote::quote! { ::core::result::Result::Err(#result) }
}

/// 生成完整的 Unt HList 类型。
///
/// 对于路径 `[A, B, C]`，生成 `Unt<A, Unt<B, Unt<C, End>>>`。
pub(crate) fn build_unt_type(paths: &[Path]) -> proc_macro2::TokenStream {
    let mut result = quote::quote! { ::struct_error::End };

    for path in paths.iter().rev() {
        result = quote::quote! { ::struct_error::Unt<#path, #result> };
    }

    result
}

/// 检查一个标识符是否是 PascalCase（首字母大写）。
pub(crate) fn is_pascal_case(ident: &syn::Ident) -> bool {
    let s = ident.to_string();
    s.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
}
