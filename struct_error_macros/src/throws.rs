//! Implementation of the `#[throws]` attribute macro.
//!
//! The most complex macro in the crate: a two-phase expansion.
//!
//! # Phase 1 — `throws` proc macro
//!
//! Generates a `macro_magic::forward_tokens!` call chain. For each error type listed in
//! `#[throws(ErrorA, ErrorB)]`, the macro forwards the type's tokens to the `__throws_cps`
//! callback, accumulating them until all types have been collected.
//!
//! # Phase 2 — `__throws_impl` function-like proc macro
//!
//! Receives the collected forwarded tokens, parses any `#[united_error]` members, deduplicates
//! and blind-sorts the error paths, generates the `Unt` HList return type, injects a local
//! `__StructErrorInto` trait, and rewrites the function body AST.
//!
//! # Communication protocol
//!
//! Phase 1 wraps each parameter in a **bracket group** `[...]` when passing to Phase 2.
//! This prevents semicolons inside forwarded tokens from interfering with parsing.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{ToTokens, quote};
use syn::parse::Parser;
use syn::visit_mut::VisitMut;

// =============================================================================
// Phase 1: #[throws] attribute macro
// =============================================================================

/// #[throws] 属性宏入口。
///
/// 语法：`#[throws(ErrorA, ErrorB)] pub fn foo() -> T { ... }`
///
/// 完整的两阶段展开协议见[模块级文档](self)。
pub(crate) fn throws(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr_ts = proc_macro2::TokenStream::from(attr);
    let item_ts = proc_macro2::TokenStream::from(item);

    // 解析属性参数列表
    let error_paths = match parse_error_paths(&attr_ts) {
        Ok(p) => p,
        Err(e) => return e.to_compile_error().into(),
    };

    // 解析被修饰的函数
    let item_fn = match syn::parse2::<syn::ItemFn>(item_ts.clone()) {
        Ok(f) => f,
        Err(e) => return e.to_compile_error().into(),
    };

    let fn_sig = &item_fn.sig;
    let fn_block = &item_fn.block;
    let fn_vis = &item_fn.vis;
    let fn_attrs = &item_fn.attrs;

    // 提取返回类型（若无显式返回类型，视为 ()）
    let ret_ty = match &fn_sig.output {
        syn::ReturnType::Default => quote! { () },
        syn::ReturnType::Type(_, ty) => ty.to_token_stream(),
    };

    // 提取函数签名（去除 -> T 部分，因为会被 __throws_impl 重写）
    let mut sig_without_ret = fn_sig.clone();
    sig_without_ret.output = syn::ReturnType::Default;

    // 阶段 1：生成 forward_tokens! 调用链。
    // 利用 struct_error crate 中导出的 __throws_cps 属性宏串联
    // macro_magic::forward_tokens!，将外部类型的 tokens 收集后传给
    // __throws_impl 完成阶段 2 展开。
    // 阶段 1：生成 forward_tokens! 调用链。
    // macro_magic::forward_tokens! 需要 4 个参数：source, target, mm_path, { extra }
    let output = if error_paths.is_empty() {
        quote! {
            ::struct_error::__throws_impl! {
                [] [#ret_ty] [#fn_block] [#sig_without_ret] [#fn_vis] [#(#fn_attrs)*]
            }
        }
    } else {
        let first_path = &error_paths[0];
        let rest_paths = &error_paths[1..];
        quote! {
            ::struct_error::macro_magic::forward_tokens! {
                #first_path,
                ::struct_error::__throws_cps,
                ::struct_error::macro_magic,
                { [@recurse] [[]] [#(#rest_paths),*] [#ret_ty] [#fn_block] [#sig_without_ret] [#fn_vis] [#(#fn_attrs)*] }
            }
        }
    };

    output.into()
}

/// 解析 #[throws(A, B, C)] 中的错误类型路径列表。
fn parse_error_paths(attr_ts: &proc_macro2::TokenStream) -> syn::Result<Vec<syn::Path>> {
    let paths = syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated
        .parse2(attr_ts.clone())?;
    Ok(paths.into_iter().collect())
}

// =============================================================================
// __throws_cps attribute macro (forward_tokens! callback target)
// =============================================================================

use syn::parse::{Parse, ParseStream};

/// Parsed representation of the `__throws_cps` attribute arguments.
///
/// Format: `__private_macro_magic_tokens_forwarded, <item>, { [@recurse] [acc] [remaining] ... }`
struct CpsAttr {
    item: syn::Item,
    extra: proc_macro2::Group,
}

impl Parse for CpsAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let _keyword: syn::Ident = input.parse()?; // __private_macro_magic_tokens_forwarded
        // macro_magic 生成的属性中，keyword 与 item 之间没有逗号
        let item: syn::Item = input.parse()?;
        let _comma: syn::Token![,] = input.parse()?;
        let extra = input.parse::<proc_macro2::Group>()?;
        Ok(Self { item, extra })
    }
}

/// __throws_cps 属性宏入口。
///
/// 由 `macro_magic::forward_tokens!` 调用，串联多个外部类型的 tokens。
/// 将 forwarded items 累加到单一 bracket group 中，所有路径收集完毕后最终调用
/// `__throws_impl`。
///
/// 此宏**不**供直接使用。
pub(crate) fn __throws_cps(attr: TokenStream, _item: TokenStream) -> TokenStream {
    let attr_ts = proc_macro2::TokenStream::from(attr);
    // eprintln!("__THROWS_CPS CALLED WITH ATTR: {}", attr_ts);

    let cps_attr = match syn::parse2::<CpsAttr>(attr_ts) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("__THROWS_CPS PARSE ERROR: {}", e);
            return e.to_compile_error().into();
        }
    };

    let forwarded_item = cps_attr.item;
    let extra_stream = cps_attr.extra.stream();

    // 从 extra 中提取 bracket groups
    let groups = extract_bracket_groups(extra_stream);
    if groups.len() < 8 {
        return syn::Error::new(
            Span::call_site(),
            "__throws_cps: expected format: { [@recurse] [acc] [remaining] [ret_ty] [body] [sig] [vis] [attrs] }",
        )
        .to_compile_error()
        .into();
    }

    let acc_ts = &groups[1];
    let remaining_ts = &groups[2];
    let ret_ty_ts = &groups[3];
    let body_ts = &groups[4];
    let sig_ts = &groups[5];
    let vis_ts = &groups[6];
    let attrs_ts = &groups[7];

    // 将 forwarded_item 追加到 acc（用圆括号包裹每个 item，避免分号与单元结构体语法冲突）
    let new_acc = if acc_ts.is_empty() {
        quote! { [(#forwarded_item)] }
    } else {
        quote! { #acc_ts(#forwarded_item) }
    };

    // 解析 remaining paths
    let remaining_paths: Vec<syn::Path> = if remaining_ts.is_empty() {
        Vec::new()
    } else {
        match syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated
            .parse2(remaining_ts.clone())
        {
            Ok(p) => p.into_iter().collect(),
            Err(e) => return e.to_compile_error().into(),
        }
    };

    if remaining_paths.is_empty() {
        // 终止：调用 __throws_impl
        let output = quote! {
            ::struct_error::__throws_impl! {
                [#new_acc] [#ret_ty_ts] [#body_ts] [#sig_ts] [#vis_ts] [#attrs_ts]
            }
        };
        // eprintln!("__THROWS_CPS OUTPUT: {}", output);
        output.into()
    } else {
        // 递归：forward 下一条路径
        let next_path = &remaining_paths[0];
        let rest_paths = &remaining_paths[1..];
        let output = quote! {
            ::struct_error::macro_magic::forward_tokens! {
                #next_path,
                ::struct_error::__throws_cps,
                ::struct_error::macro_magic,
                { [@recurse] [#new_acc] [#(#rest_paths),*] [#ret_ty_ts] [#body_ts] [#sig_ts] [#vis_ts] [#attrs_ts] }
            }
        };
        output.into()
    }
}

// =============================================================================
// Phase 2: __throws_impl function-like proc macro
// =============================================================================

/// __throws_impl function-like proc macro 入口。
///
/// 语法：`__throws_impl! { [forwarded_tokens...] [ret_ty] [body] [sig] [vis] [attrs] }`
///
/// 接收所有 forwarded tokens 以及原始函数元数据，然后执行真正的签名重写与
/// AST 转换（详见[模块级文档](self)）。
///
/// 此宏**不**供直接使用。
pub(crate) fn __throws_impl(input: TokenStream) -> TokenStream {
    let input_ts = proc_macro2::TokenStream::from(input);

    // 按 Bracket Group 提取参数
    let groups = extract_bracket_groups(input_ts);

    if groups.len() < 6 {
        return syn::Error::new(
            Span::call_site(),
            "__throws_impl: expected format: [tokens...] [ret_ty] [body] [sig] [vis] [attrs]",
        )
        .to_compile_error()
        .into();
    }

    // groups[0] = forwarded tokens 列表（每个 item 用 bracket group 包裹）
    // groups[1] = ret_ty
    // groups[2] = body
    // groups[3] = sig
    // groups[4] = vis
    // groups[5] = attrs
    let forwarded_tokens_group = &groups[0];
    let ret_ty_ts = &groups[1];
    let body_ts = &groups[2];
    let sig_ts = &groups[3];
    let vis_ts = &groups[4];
    let attrs_ts = &groups[5];

    // 解析 forwarded tokens 列表（从圆括号 group 中提取每个 item）
    let forwarded_items = extract_paren_groups(forwarded_tokens_group.clone());

    // 解析 ret_ty
    let ret_ty: syn::Type = match syn::parse2(ret_ty_ts.clone()) {
        Ok(t) => t,
        Err(e) => return e.to_compile_error().into(),
    };

    // 解析 body
    let mut fn_block: syn::Block = match syn::parse2(body_ts.clone()) {
        Ok(b) => b,
        Err(e) => return e.to_compile_error().into(),
    };

    // 解析 sig
    let mut fn_sig: syn::Signature = match syn::parse2(sig_ts.clone()) {
        Ok(s) => s,
        Err(e) => return e.to_compile_error().into(),
    };

    // 解析 vis
    let fn_vis: syn::Visibility = match syn::parse2(vis_ts.clone()) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error().into(),
    };

    // 解析 attrs（尝试作为属性列表解析）
    let fn_attrs: Vec<syn::Attribute> = if attrs_ts.is_empty() {
        Vec::new()
    } else {
        // 用 syn::File 的 attrs 解析方式：在前面加一个 dummy item
        let dummy = quote! { #attrs_ts fn __dummy() {} };
        match syn::parse2::<syn::ItemFn>(dummy) {
            Ok(item) => item.attrs,
            Err(_) => Vec::new(),
        }
    };

    // 解析 forwarded items，提取错误类型
    let mut all_error_paths: Vec<syn::Path> = Vec::new();

    for tokens in &forwarded_items {
        if tokens.is_empty() {
            continue;
        }

        // 尝试解析为 ItemStruct（锚点结构体）
        if let Ok(item_struct) = syn::parse2::<syn::ItemStruct>(tokens.clone()) {
            // 查找 __struct_error_members 属性（支持单段或多段路径，如 ::struct_error::__struct_error_members）
            let mut found_members = false;
            for attr in &item_struct.attrs {
                if attr
                    .path()
                    .segments
                    .last()
                    .map(|s| s.ident == "__struct_error_members")
                    .unwrap_or(false)
                {
                    found_members = true;
                    match attr.parse_args_with(
                        syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
                    ) {
                        Ok(members) => {
                            all_error_paths.extend(members);
                        }
                        Err(e) => return e.to_compile_error().into(),
                    }
                }
            }

            if !found_members {
                // 没有 __struct_error_members：视为单个基础错误
                let path = syn::Path::from(item_struct.ident.clone());
                all_error_paths.push(path);
            }
        } else {
            // 无法解析为 struct，尝试直接解析为 Path
            if let Ok(path) = syn::parse2::<syn::Path>(tokens.clone()) {
                all_error_paths.push(path);
            }
        }
    }

    // 去重并排序
    let sorted_paths = crate::sort::sort_paths(all_error_paths);
    let unique_paths = crate::sort::dedup_paths(sorted_paths);

    // 生成 Unt HList 类型
    let unt_type = crate::sort::build_unt_type(&unique_paths);

    // 构建错误类型映射（路径 -> 排序索引）
    let _error_index_map: std::collections::HashMap<String, usize> = unique_paths
        .iter()
        .enumerate()
        .map(|(i, p)| (crate::sort::path_to_string(p), i))
        .collect();

    // 生成局部 __StructErrorInto trait 及实现
    let into_trait = generate_into_trait(&unique_paths);

    // 重写函数体 AST
    let mut rewriter = ThrowsRewriter;
    rewriter.visit_block_mut(&mut fn_block);

    // 重写函数签名
    fn_sig.output = syn::ReturnType::Type(
        syn::Token![->](Span::call_site()),
        syn::parse2(quote! { ::core::result::Result<#ret_ty, #unt_type> }).unwrap(),
    );

    // 处理隐式 () 返回值：若原函数无返回类型，在末尾注入 Ok(())
    let is_unit_ret = matches!(ret_ty, syn::Type::Tuple(t) if t.elems.is_empty());
    if is_unit_ret {
        // 检查 block 的最后一个语句是否是表达式
        let needs_ok = if let Some(last) = fn_block.stmts.last() {
            !matches!(last, syn::Stmt::Expr(_, None))
        } else {
            true
        };
        if needs_ok {
            fn_block.stmts.push(
                syn::parse2(quote! {
                    ::core::result::Result::Ok(())
                })
                .unwrap(),
            );
        }
    }

    // 将 __StructErrorInto trait 及实现注入函数体开头，避免模块级命名冲突
    let into_trait_items = match syn::parse2::<syn::File>(into_trait) {
        Ok(file) => file.items,
        Err(_) => Vec::new(),
    };
    let mut trait_stmts: Vec<syn::Stmt> =
        into_trait_items.into_iter().map(syn::Stmt::Item).collect();
    trait_stmts.append(&mut fn_block.stmts);
    fn_block.stmts = trait_stmts;

    // 构建最终函数
    let expanded = quote! {
        #(#fn_attrs)*
        #fn_vis #fn_sig #fn_block
    };

    expanded.into()
}

/// Extracts the contents of all bracket groups `[...]` from a TokenStream.
fn extract_bracket_groups(ts: proc_macro2::TokenStream) -> Vec<proc_macro2::TokenStream> {
    let mut result = Vec::new();
    for tt in ts {
        if let proc_macro2::TokenTree::Group(g) = tt
            && g.delimiter() == proc_macro2::Delimiter::Bracket
        {
            result.push(g.stream());
        }
    }
    result
}

/// Extracts the contents of all parenthesis groups `(...)` from a TokenStream.
fn extract_paren_groups(ts: proc_macro2::TokenStream) -> Vec<proc_macro2::TokenStream> {
    let mut result = Vec::new();
    for tt in ts {
        if let proc_macro2::TokenTree::Group(g) = tt
            && g.delimiter() == proc_macro2::Delimiter::Parenthesis
        {
            result.push(g.stream());
        }
    }
    result
}

/// Generates the local `__StructErrorInto` trait and implementations for each error type.
///
/// Also generates implementations for `End` and a recursive impl for `Unt<H, T>`, allowing
/// `?` propagation between functions with different (but overlapping) `throws` lists.
fn generate_into_trait(paths: &[syn::Path]) -> proc_macro2::TokenStream {
    if paths.is_empty() {
        return quote! {};
    }

    let unt_type = crate::sort::build_unt_type(paths);

    let impls: Vec<_> = paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            let nested = crate::sort::build_unt_nesting(index, paths.len(), &quote! { self });
            quote! {
                impl __StructErrorInto for #path {
                    fn into_unt(self) -> #unt_type {
                        #nested
                    }
                }
            }
        })
        .collect();

    quote! {
        trait __StructErrorInto {
            fn into_unt(self) -> #unt_type;
        }
        #(#impls)*

        // 为 End 实现（uninhabited，永远不可达）
        impl __StructErrorInto for ::struct_error::End {
            fn into_unt(self) -> #unt_type {
                match self {}
            }
        }

        // 为所有 Unt<H, T> 实现递归转换。
        // 这使得不同 throws 列表（子集关系）的函数间也能使用 ? 传播，
        // 只要底层错误类型在当前 throws 列表中有对应的 __StructErrorInto 实现。
        impl<H, T> __StructErrorInto for ::struct_error::Unt<H, T>
        where
            H: __StructErrorInto,
            T: __StructErrorInto,
        {
            fn into_unt(self) -> #unt_type {
                match self {
                    ::struct_error::Unt::Here(h) => h.into_unt(),
                    ::struct_error::Unt::There(t) => t.into_unt(),
                }
            }
        }
    }
}

// =============================================================================
// AST Rewriter: VisitMut implementation
// =============================================================================

/// AST rewriter for the body of a `#[throws]` function.
///
/// Intercepts `?` operators and wraps `return` / tail expressions in `Ok(...)`.
/// Only `throw!` may actively propagate an error.
struct ThrowsRewriter;

impl VisitMut for ThrowsRewriter {
    // 保护内部闭包：不进入闭包内部
    fn visit_expr_closure_mut(&mut self, _node: &mut syn::ExprClosure) {
        // Stop：不遍历闭包内部
    }

    // 保护内部 async 块
    fn visit_expr_async_mut(&mut self, _node: &mut syn::ExprAsync) {
        // Stop
    }

    // 保护内部函数定义
    fn visit_item_fn_mut(&mut self, _node: &mut syn::ItemFn) {
        // Stop
    }

    // 保护内部模块
    fn visit_item_mod_mut(&mut self, _node: &mut syn::ItemMod) {
        // Stop
    }

    // 拦截 ? 操作符：替换整个 Expr::Try 节点为 match 表达式
    fn visit_expr_mut(&mut self, node: &mut syn::Expr) {
        if let syn::Expr::Try(try_expr) = node {
            // 先递归处理内部表达式
            self.visit_expr_mut(&mut try_expr.expr);
            let expr = &try_expr.expr;
            // 将 expr? 替换为 match 表达式，彻底消除 ? 节点
            *node = syn::parse2(quote! {
                match #expr {
                    ::core::result::Result::Ok(v) => v,
                    ::core::result::Result::Err(e) => {
                        return ::core::result::Result::Err(__StructErrorInto::into_unt(e))
                    }
                }
            })
            .unwrap();
        } else {
            syn::visit_mut::visit_expr_mut(self, node);
        }
    }

    // 拦截 return 语句
    fn visit_stmt_mut(&mut self, node: &mut syn::Stmt) {
        // 先递归处理（默认行为）
        syn::visit_mut::visit_stmt_mut(self, node);

        // 检查是否是 return <expr>;
        if let syn::Stmt::Expr(syn::Expr::Return(ret), _) = node
            && let Some(expr) = &mut ret.expr
        {
            let wrapped = self.wrap_expr(expr);
            **expr = wrapped;
        }
    }

    // 拦截尾部表达式（Block 的最后一个表达式语句）
    fn visit_block_mut(&mut self, node: &mut syn::Block) {
        // 先递归处理所有语句（默认行为）
        for stmt in &mut node.stmts {
            self.visit_stmt_mut(stmt);
        }

        // 处理尾部表达式
        if let Some(last) = node.stmts.last_mut()
            && let syn::Stmt::Expr(expr, semi) = last
            && semi.is_none()
        {
            // 这是尾部表达式，需要包装
            let wrapped = self.wrap_tail_expr(expr);
            *expr = wrapped;
        }
    }
}

impl ThrowsRewriter {
    /// 包装 return 中的表达式：统一包装为 Ok(...)。
    /// 只有 throw! 宏可以主动抛出错误，return 永远返回成功值。
    fn wrap_expr(&self, expr: &syn::Expr) -> syn::Expr {
        // 防重入：如果用户已显式写了 Ok(...) 或 Err(...)，直接透传
        if crate::utils::is_ok_expr(expr) || crate::utils::is_err_expr(expr) {
            return expr.clone();
        }

        // 兜底：所有 return 表达式统一包装为 Ok(...)
        syn::parse2(quote! {
            ::core::result::Result::Ok(#expr)
        })
        .unwrap()
    }

    /// 包装尾部表达式：与 return 使用相同逻辑。
    fn wrap_tail_expr(&self, expr: &syn::Expr) -> syn::Expr {
        self.wrap_expr(expr)
    }
}
