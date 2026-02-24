pub trait Describable {
    fn describe(&self) -> String;
}

pub trait AsyncFetch {
    async fn fetch(&self) -> String;
}

