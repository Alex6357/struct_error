//! AST traversal and TokenStream processing helpers.

/// 检查一个表达式是否看起来像 `Ok(...)` 构造函数。
pub(crate) fn is_ok_expr(expr: &syn::Expr) -> bool {
    if let syn::Expr::Call(call) = expr
        && let syn::Expr::Path(path) = &*call.func
    {
        return is_path_ok(&path.path);
    }
    false
}

/// 检查一个表达式是否看起来像 `Err(...)` 构造函数。
pub(crate) fn is_err_expr(expr: &syn::Expr) -> bool {
    if let syn::Expr::Call(call) = expr
        && let syn::Expr::Path(path) = &*call.func
    {
        return is_path_err(&path.path);
    }
    false
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
