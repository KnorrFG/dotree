use std::path::PathBuf;

use once_cell::sync::OnceCell;

use crate::parser::Settings;

static LOCAL_CONF_DIR: OnceCell<Option<PathBuf>> = OnceCell::new();
static SETTINGS: OnceCell<Settings> = OnceCell::new();

pub fn init(local_conf_dir: Option<PathBuf>, settings: Settings) {
    LOCAL_CONF_DIR
        .set(local_conf_dir)
        .expect("initiating rt conf twice");
    SETTINGS.set(settings).unwrap();
}

pub fn local_conf_dir() -> Option<&'static PathBuf> {
    LOCAL_CONF_DIR.get().expect("missing initiation").as_ref()
}

pub fn settings() -> &'static Settings {
    SETTINGS.get().expect("missing initiation")
}
