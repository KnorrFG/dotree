use std::{fs, process::exit};

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use console::Term;
use dotree::{
    core::run,
    parser::{self, Node},
};

fn main() -> Result<()> {
    pretty_env_logger::init();
    let args = Args::parse();

    let conf_path = dirs::config_dir()
        .ok_or(anyhow!("Couldn't determin config dir"))?
        .join("dotree.dt");
    if !conf_path.exists() {
        eprintln!(
            "Expected config file at {}, but couldn't find it. Please create one.",
            conf_path.display()
        );
        exit(1);
    }

    let conf_src = fs::read_to_string(conf_path).context("loading config")?;
    let conf = parser::parse(&conf_src).context("Parsing Config")?;
    let term = Term::stdout();
    term.hide_cursor()?;
    let res = run(&Node::Menu(conf), args.input.as_deref());
    if let Err(e) = term.show_cursor() {
        eprintln!("Warning, couldn't show cursor again:\n{e:?}");
    }
    res
}

#[derive(Parser)]
struct Args {
    /// Input that will be process character by character, as if it was entered
    input: Option<String>,
}
