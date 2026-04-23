//! Implementation of the `#[throws]` attribute macro.
//!
//! Rewrites a function to implicitly return `Result<T, Unt<...>>` and intercepts `?`
//! operators.
//!
//! # How it works
//!
//! 1. Parses the error type paths from `#[throws(ErrorA, ErrorB)]`.
//! 2. Resolves paths via the global registry — united errors are automatically expanded
//!    into their constituent members.
//! 3. Blind-sorts and deduplicates the final error path list.
//! 4. Generates the `Unt` HList return type and a local `__StructErrorInto` trait.
//! 5. Rewrites the function body AST to intercept `?` operators and wrap returns in
//!    `Ok(...)`.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{ToTokens, quote};
use syn::parse::Parser;
use syn::visit_mut::VisitMut;

/// #[throws] 属性宏入口。
///
/// 语法：`#[throws(ErrorA, ErrorB)] pub fn foo() -> T { ... }`
pub(crate) fn throws(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr_ts = proc_macro2::TokenStream::from(attr);
    let item_ts = proc_macro2::TokenStream::from(item);

    // 解析属性参数列表
    let error_paths = match parse_error_paths(&attr_ts) {
        Ok(p) => p,
        Err(e) => return e.to_compile_error().into(),
    };

    // 解析被修饰的函数
    let mut item_fn = match syn::parse2::<syn::ItemFn>(item_ts.clone()) {
        Ok(f) => f,
        Err(e) => return e.to_compile_error().into(),
    };

    // 提取返回类型（若无显式返回类型，视为 ()）
    let ret_ty = match &item_fn.sig.output {
        syn::ReturnType::Default => quote! { () },
        syn::ReturnType::Type(_, ty) => ty.to_token_stream(),
    };

    // 通过全局状态解析联合类型，获取最终错误路径列表
    let expanded_paths = crate::registry::resolve_error_paths(&error_paths);

    // 去重并排序
    let sorted_paths = crate::sort::sort_paths(expanded_paths);
    let unique_paths = crate::sort::dedup_paths(sorted_paths);

    // 生成 Unt HList 类型
    let unt_type = crate::sort::build_unt_type(&unique_paths);

    // 生成局部 __StructErrorInto trait 及实现
    let into_trait = generate_into_trait(&unique_paths);

    // 重写函数体 AST
    let mut rewriter = ThrowsRewriter;
    rewriter.visit_block_mut(&mut item_fn.block);

    // 单独处理顶层 block 的尾部表达式，避免嵌套 block 被重复包装
    if let Some(last) = item_fn.block.stmts.last_mut()
        && let syn::Stmt::Expr(expr, semi) = last
        && semi.is_none()
    {
        let wrapped = rewriter.wrap_tail_expr(expr);
        *expr = wrapped;
    }

    // 重写函数签名
    item_fn.sig.output = syn::ReturnType::Type(
        syn::Token![->](Span::call_site()),
        syn::parse2(quote! { ::core::result::Result<#ret_ty, #unt_type> }).unwrap(),
    );

    // 处理隐式 () 返回值：若原函数无返回类型，在末尾注入 Ok(())
    let is_unit_ret = matches!(
        syn::parse2::<syn::Type>(ret_ty.clone()),
        Ok(syn::Type::Tuple(t)) if t.elems.is_empty()
    );
    if is_unit_ret {
        let needs_ok = if let Some(last) = item_fn.block.stmts.last() {
            !matches!(last, syn::Stmt::Expr(_, None))
        } else {
            true
        };
        if needs_ok {
            item_fn.block.stmts.push(
                syn::parse2(quote! {
                    ::core::result::Result::Ok(())
                })
                .unwrap(),
            );
        }
    }

    // 将 __StructErrorInto trait 及实现注入函数体开头
    let into_trait_items = match syn::parse2::<syn::File>(into_trait) {
        Ok(file) => file.items,
        Err(_) => Vec::new(),
    };
    let mut trait_stmts: Vec<syn::Stmt> =
        into_trait_items.into_iter().map(syn::Stmt::Item).collect();
    trait_stmts.append(&mut item_fn.block.stmts);
    item_fn.block.stmts = trait_stmts;

    // 构建最终函数
    let expanded = quote! {
        #item_fn
    };

    expanded.into()
}

/// 解析 #[throws(A, B, C)] 中的错误类型路径列表。
fn parse_error_paths(attr_ts: &proc_macro2::TokenStream) -> syn::Result<Vec<syn::Path>> {
    let paths = syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated
        .parse2(attr_ts.clone())?;
    Ok(paths.into_iter().collect())
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

        // 注意：不在此处包装尾部表达式，以避免对嵌套 block（如 match arm、if 分支）
        // 进行重复包装。顶层 block 的尾部表达式包装在 throws() 函数中统一处理。
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
