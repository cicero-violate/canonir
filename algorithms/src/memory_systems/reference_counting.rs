use std::rc::Rc;

pub fn new_ref<T>(value: T) -> Rc<T> {
    Rc::new(value)
}

pub fn clone_ref<T>(rc: &Rc<T>) -> Rc<T> {
    Rc::clone(rc)
}

pub fn strong_count<T>(rc: &Rc<T>) -> usize {
    Rc::strong_count(rc)
}
