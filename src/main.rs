use std::collections::HashMap;

use anyhow::Result;
use clap::Parser;
use console::Term;
use dotree::core::{run, Menu, Node};

fn main() -> Result<()> {
    pretty_env_logger::init();
    let conf = mk_example_conf();
    let args = Args::parse();
    let term = Term::stderr();
    term.hide_cursor()?;
    let res = run(&conf, args.input.as_ref().map(String::as_str));
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

fn mk_example_conf() -> Menu {
    let git_commit = Menu {
        name: "commit".into(),
        entries: HashMap::from([
            ("a".into(), Node::Command("git commit -a".into())),
            ("m".into(), Node::Command("git commit --amend".into())),
        ]),
    };

    let git_menu = Menu {
        name: "git".into(),
        entries: HashMap::from([
            ("aa".into(), Node::Command("git add -A .".into())),
            ("d".into(), Node::Command("git diff".into())),
            ("s".into(), Node::Command("git status".into())),
            ("c".into(), Node::Menu(git_commit)),
        ]),
    };

    let custom_commands = Menu {
        name: "custom commands".into(),
        entries: HashMap::from([
            ("m".into(), Node::Command("pandoc \"${{file}}\" -c ~/Sync/share/pandoc.css --toc --standalone --embed-resources -so \"${${{file}}.md}.html".into())),
        ]),
    };

    Menu {
        name: "Root".into(),
        entries: HashMap::from([
            ("g".into(), Node::Menu(git_menu)),
            ("c".into(), Node::Menu(custom_commands)),
        ]),
    }
}
