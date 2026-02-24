use std::sync::mpsc::{channel, Sender};
use std::thread;

pub fn spawn_actor<T: Send + 'static, F: Fn(T) + Send + 'static>(handler: F) -> Sender<T> {
    let (tx, rx) = channel();
    thread::spawn(move || {
        for msg in rx {
            handler(msg);
        }
    });
    tx
}
