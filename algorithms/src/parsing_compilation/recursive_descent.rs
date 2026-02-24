use super::ast::Expr;

pub fn parse_expression(tokens: &[&str]) -> Option<Expr> {
    if tokens.len() == 1 {
        return tokens[0].parse::<i64>().ok().map(Expr::Number);
    }
    if tokens.len() == 3 {
        let left = tokens[0].parse::<i64>().ok()?;
        let right = tokens[2].parse::<i64>().ok()?;
        return match tokens[1] {
            "+" => Some(Expr::Add(Box::new(Expr::Number(left)), Box::new(Expr::Number(right)))),
            "-" => Some(Expr::Sub(Box::new(Expr::Number(left)), Box::new(Expr::Number(right)))),
            _ => None,
        };
    }
    None
}
