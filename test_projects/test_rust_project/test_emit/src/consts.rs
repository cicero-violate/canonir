pub const MAX_SCORE: u32 = 1000;

pub const APP_NAME: &str = "test_rust_project";

#[allow(non_upper_case_globals)]
pub static mut CALL_COUNT: u32 = 0;

pub fn boot() {
    println!("boot: {} v{}", APP_NAME, MAX_SCORE);
}

