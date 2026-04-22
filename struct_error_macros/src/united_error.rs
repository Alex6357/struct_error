//! Implementation of the `#[united_error]` attribute macro.
//!
//! Defines compile-time aliases for error sets. The generated struct is a zero-sized type
//! (ZST) annotated with `#[__struct_error_members(...)]` and registered in a global
//! registry, allowing `#[throws]` and `match_error!` to expand the alias automatically.

use proc_macro::TokenStream;
use quote::quote;
use syn::Token;
use syn::parse::Parser;
use syn::punctuated::Punctuated;

/// #[united_error] 属性宏入口。
pub(crate) fn united_error(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr_ts = proc_macro2::TokenStream::from(attr);
    let item_ts = proc_macro2::TokenStream::from(item);

    // 解析被修饰的结构体
    let item_ast = match syn::parse2::<syn::ItemStruct>(item_ts.clone()) {
        Ok(s) => s,
        Err(e) => return e.to_compile_error().into(),
    };

    let ident = &item_ast.ident;
    let vis = &item_ast.vis;

    // 解析属性参数列表：#[united_error(NotFound, Timeout)]
    let members = match parse_members(&attr_ts) {
        Ok(m) => m,
        Err(e) => return e.to_compile_error().into(),
    };

    // 注册到全局状态
    let members_str: Vec<String> = members.iter().map(crate::sort::path_to_string).collect();
    crate::registry::register_united_error(&ident.to_string(), members_str);

    // 生成 __struct_error_members 属性，编码成员列表（使用全限定路径避免用户导入）
    let members_attr = quote! {
        #[::struct_error::__struct_error_members(#(#members),*)]
    };

    let expanded = quote! {
        #members_attr
        #vis struct #ident;
    };

    expanded.into()
}

/// 解析 #[united_error(A, B, C)] 中的成员类型路径列表。
fn parse_members(attr_ts: &proc_macro2::TokenStream) -> syn::Result<Vec<syn::Path>> {
    // syn 可以直接解析 Punctuated<Path, Token![,]>
    let paths = Punctuated::<syn::Path, Token![,]>::parse_terminated.parse2(attr_ts.clone())?;
    Ok(paths.into_iter().collect())
}
