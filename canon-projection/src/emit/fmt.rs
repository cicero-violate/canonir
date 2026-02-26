use canon::ir::CanonIR;
use canon::node::flags;

pub fn vis_token(f: u32) -> &'static str {
    if (f & flags::PUB) != 0 {
        "pub "
    } else if (f & flags::PUB_CRATE) != 0 {
        "pub(crate) "
    } else if (f & flags::PUB_SUPER) != 0 {
        "pub(super) "
    } else {
        ""
    }
}

pub fn normalize_use_path<'a>(path: &'a str, _ir: &CanonIR) -> std::borrow::Cow<'a, str> {
    std::borrow::Cow::Borrowed(path)
}
