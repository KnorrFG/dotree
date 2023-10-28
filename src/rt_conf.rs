use std::path::PathBuf;

use once_cell::sync::OnceCell;

static LOCAL_CONF_DIR: OnceCell<Option<PathBuf>> = OnceCell::new();

pub fn init(local_conf_dir: Option<PathBuf>) {
    LOCAL_CONF_DIR
        .set(local_conf_dir)
        .expect("initiating rt conf twice");
}

pub fn local_conf_dir() -> Option<&'static PathBuf> {
    LOCAL_CONF_DIR.get().expect("missing initiation").as_ref()
}
