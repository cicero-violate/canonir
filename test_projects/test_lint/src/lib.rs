// test_api/src/lib.rs
pub mod api {
    #[expect(api_traits_only, reason = "exercise lint signal for API structs without traits")]
    pub struct Bad; // should emit signal
    pub trait Good {} // allowed
}
