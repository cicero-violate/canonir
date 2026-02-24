use crate::traits::Describable;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct User {
    pub name: String,
    pub score: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Active,
    Inactive,
    Banned(String),
}

impl User {
    pub fn new(name: &str, score: u32) -> Self {
        Self { name: name.to_string(), score }
    }
    pub fn status(&self) -> Status {
        if self.score > 0 { Status::Active } else { Status::Inactive }
    }
}

impl Describable for User {
    fn describe(&self) -> String {
        format!("User({}, {})", self.name, self.score)
    }
}

#[derive(Debug, Clone)]
pub struct Score {
    pub value: u32,
}

impl Describable for Score {
    fn describe(&self) -> String {
        format!("Score({})", self.value)
    }
}

pub type UserVec = Vec<User>;

