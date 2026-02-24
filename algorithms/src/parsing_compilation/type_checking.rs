use super::ast::Expr;

pub fn type_check(expr: &Expr) -> bool {
    match expr {
        Expr::Number(_) => true,
        Expr::Add(a, b) | Expr::Sub(a, b) => type_check(a) && type_check(b),
    }
}
