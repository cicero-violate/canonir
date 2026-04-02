#[cfg(not(canon_rustc_typegen))]
pub mod tlog_writer;
#[cfg(not(canon_rustc_typegen))]
pub mod panic_capture;
#[cfg(not(canon_rustc_typegen))]
pub mod warnings;

#[cfg(not(canon_rustc_typegen))]
pub use tlog_writer::{emit_ir_tlog, TlogWriter};
#[cfg(not(canon_rustc_typegen))]
pub use panic_capture::{append_panic_record, install_panic_hook, set_panic_log_root};
#[cfg(not(canon_rustc_typegen))]
pub use warnings::{append_rustc_log, append_rustc_warning, append_rustc_warning_with_root};
