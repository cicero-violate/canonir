pub type Result1231<T> = std::result::Result<T, String>;

pub fn compute_result(a: i32, b: i32) -> Result1231<i32> {
    if b == 0 {
        return Err("division by zero".into());
    } else {
        return Ok(a / b);
    }
}

pub fn combine_results(a: &str, b: &str) -> Result1231<i32> {
    let x = a.parse::<i32>().map_err(|e| e.to_string())?;
    let y = b.parse::<i32>().map_err(|e| e.to_string())?;
    compute_result(x, y)
}

