use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct PanicRecord {
    pub message: String,
    pub backtrace: String,
}
