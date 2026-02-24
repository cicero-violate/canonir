use crate::data::model::User;

pub fn run() {
    let user = User::new("Cheese", 42);
    println!("User: {:?}", user);
    let status = user.status();
    println!("Status: {:?}", status);
}

pub unsafe fn unsafe_low_level(ptr: *const u32) -> u32 {
    unsafe { *ptr }
}

pub async fn fetch_data(url: &str) -> String {
    format!("fetched: {}", url)
}

fn private_helper(x: u32) -> u32 {
    x * 2
}

fn dead_function() {
    println!("I am never called")
}

