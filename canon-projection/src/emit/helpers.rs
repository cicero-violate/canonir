pub trait Emit {
    fn emit(&self, pad: &str) -> String;
}
