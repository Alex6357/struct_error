//! Implementation of the `#[error]` attribute macro.
//!
//! Automatically derives `Debug`, `Display`, and `std::error::Error` for the annotated struct.
//! Also registers the struct identifier in a global registry so other macros
//! (`#[throws]` and `match_error!`) can resolve and expand it automatically.

use proc_macro::TokenStream;
use quote::{ToTokens, quote};

/// #[error] 属性宏入口。
pub(crate) fn error(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr_ts = proc_macro2::TokenStream::from(attr);
    let item_ts = proc_macro2::TokenStream::from(item);

    // 解析被修饰的结构体
    let item_ast = match syn::parse2::<syn::ItemStruct>(item_ts.clone()) {
        Ok(s) => s,
        Err(e) => return e.to_compile_error().into(),
    };

    // 解析属性参数：#[error("format", args...)] 或 #[error]
    let (format_str, format_args) = parse_error_attr(&attr_ts, &item_ast.ident);

    // 生成 Display 实现
    let display_impl = generate_display(&item_ast, &format_str, &format_args);

    // 生成 Error 实现（支持 #[error_source]）
    let error_impl = generate_error(&item_ast);

    // 生成 Debug 派生
    let derive_debug = quote! { #[derive(::core::fmt::Debug)] };

    // 提取结构体定义（去除 #[error] 属性本身以及字段上的 #[error_source]，避免递归和未知属性报错）
    let struct_def = strip_error_attrs(&item_ast);

    // 注册到全局状态
    crate::registry::register_error_type(&item_ast.ident.to_string());

    let expanded = quote! {
        #derive_debug
        #struct_def

        #display_impl
        #error_impl
    };

    expanded.into()
}

/// 解析 #[error] 属性参数。
/// - #[error] → 默认回退：打印结构体名字
/// - #[error("format", arg1, arg2, ...)] → 提取格式字符串和参数
fn parse_error_attr(
    attr_ts: &proc_macro2::TokenStream,
    struct_name: &syn::Ident,
) -> (syn::LitStr, proc_macro2::TokenStream) {
    // 尝试解析为函数调用形式：error("format", args...)
    // 或者直接是字面量：error("format")
    let tokens = attr_ts.clone().into_iter().collect::<Vec<_>>();

    if tokens.is_empty() {
        // 无参数：默认回退
        let default_msg = struct_name.to_string();
        return (
            syn::LitStr::new(&default_msg, struct_name.span()),
            proc_macro2::TokenStream::new(),
        );
    }

    // 尝试解析为 ("format", args...) 或 "format"
    // 简单策略：提取第一个字符串字面量作为格式，剩余作为参数
    let mut iter = tokens.into_iter().peekable();

    // 跳过可能的括号
    let first = iter.next();
    let second = iter.next();

    let (format_lit, rest): (syn::LitStr, proc_macro2::TokenStream) = match (first, second) {
        (Some(proc_macro2::TokenTree::Literal(lit)), _) => {
            let s = lit.to_string();
            // 去除字符串两端的引号
            let inner = s
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(&s);
            let lit_str = syn::LitStr::new(inner, lit.span());
            let rest: proc_macro2::TokenStream = iter.collect();
            (lit_str, rest)
        }
        (Some(proc_macro2::TokenTree::Group(g)), _) => {
            // 可能是 ("format", args...) 形式
            let mut inner_iter = g.stream().into_iter().peekable();
            let first_inner = inner_iter.next();
            let second_inner = inner_iter.next();

            match (first_inner, second_inner) {
                (Some(proc_macro2::TokenTree::Literal(lit)), _) => {
                    let s = lit.to_string();
                    let inner = s
                        .strip_prefix('"')
                        .and_then(|s| s.strip_suffix('"'))
                        .unwrap_or(&s);
                    let lit_str = syn::LitStr::new(inner, lit.span());
                    let rest: proc_macro2::TokenStream = inner_iter.collect();
                    (lit_str, rest)
                }
                _ => {
                    let default_msg = struct_name.to_string();
                    (
                        syn::LitStr::new(&default_msg, struct_name.span()),
                        proc_macro2::TokenStream::new(),
                    )
                }
            }
        }
        _ => {
            let default_msg = struct_name.to_string();
            (
                syn::LitStr::new(&default_msg, struct_name.span()),
                proc_macro2::TokenStream::new(),
            )
        }
    };

    (format_lit, rest)
}

/// 生成 std::fmt::Display 实现。
fn generate_display(
    item_ast: &syn::ItemStruct,
    format_str: &syn::LitStr,
    format_args: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let ident = &item_ast.ident;
    let (impl_generics, ty_generics, where_clause) = item_ast.generics.split_for_impl();

    if format_args.is_empty() {
        quote! {
            impl #impl_generics ::core::fmt::Display for #ident #ty_generics #where_clause {
                fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    write!(f, #format_str)
                }
            }
        }
    } else {
        quote! {
            impl #impl_generics ::core::fmt::Display for #ident #ty_generics #where_clause {
                fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    write!(f, #format_str, #format_args)
                }
            }
        }
    }
}

/// 生成 std::error::Error 实现。
/// 如果字段标记了 #[error_source]，则实现 source() 方法返回该字段。
fn generate_error(item_ast: &syn::ItemStruct) -> proc_macro2::TokenStream {
    let ident = &item_ast.ident;
    let (impl_generics, ty_generics, where_clause) = item_ast.generics.split_for_impl();

    // 查找标记了 #[error_source] 的字段
    let mut source_field = None;
    for field in item_ast.fields.iter() {
        for attr in &field.attrs {
            if attr.path().is_ident("error_source") {
                source_field = Some(field);
            }
        }
    }

    if let Some(field) = source_field {
        let field_ident = field.ident.as_ref().unwrap();
        quote! {
            impl #impl_generics ::core::error::Error for #ident #ty_generics #where_clause {
                fn source(&self) -> Option<&(dyn ::core::error::Error + 'static)> {
                    Some(&self.#field_ident)
                }
            }
        }
    } else {
        quote! {
            impl #impl_generics ::core::error::Error for #ident #ty_generics #where_clause {}
        }
    }
}

/// 去除结构体定义上的 #[error] 属性以及字段上的 #[error_source]，
/// 避免递归应用和未知属性残留。
fn strip_error_attrs(item_ast: &syn::ItemStruct) -> proc_macro2::TokenStream {
    let mut cleaned = item_ast.clone();
    // 去除结构体定义上的 #[error] 属性
    cleaned.attrs.retain(|attr| !attr.path().is_ident("error"));
    // 去除字段上的 #[error_source] 属性
    for field in cleaned.fields.iter_mut() {
        field
            .attrs
            .retain(|attr| !attr.path().is_ident("error_source"));
    }
    cleaned.to_token_stream()
}
