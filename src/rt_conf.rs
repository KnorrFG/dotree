use std::path::PathBuf;

use once_cell::sync::OnceCell;

use crate::parser::ShellDef;

static LOCAL_CONF_DIR: OnceCell<Option<PathBuf>> = OnceCell::new();
static SHELL: OnceCell<ShellDef> = OnceCell::new();

pub fn init(local_conf_dir: Option<PathBuf>, shell: ShellDef) {
    LOCAL_CONF_DIR
        .set(local_conf_dir)
        .expect("initiating rt conf twice");
    SHELL.set(shell).unwrap();
}

pub fn local_conf_dir() -> Option<&'static PathBuf> {
    LOCAL_CONF_DIR.get().expect("missing initiation").as_ref()
}

pub fn shell_def() -> &'static ShellDef {
    SHELL.get().expect("missing initiation")
}
