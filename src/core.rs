use anyhow::{anyhow, Context, Result};
use console::{pad_str, Alignment, Term};
use dialoguer::Input;
use log::debug;
use std::io::Write;
use std::{collections::HashMap, env, process::exit};

use crate::outproxy::OutProxy;
use crate::parser::{self, Menu};

#[derive(Debug, Clone, Copy)]
pub enum NodeRef<'a> {
    Menu(&'a parser::Menu),
    Command(&'a parser::Command),
}

#[derive(Debug, Clone)]
pub enum Submenus<'a, 'b> {
    Exact(NodeRef<'a>, &'b [char]),
    Incomplete,
    None,
}

pub fn run(root: &parser::Menu, input: Option<&str>) -> Result<()> {
    let mut current_menu = root;
    let root_node = NodeRef::Menu(root);
    let mut current_input = if let Some(input) = input {
        input.chars().collect()
    } else {
        vec![]
    };
    let term = Term::stdout();
    let mut out_proxy = OutProxy::new();

    let cur_node = root_node.follow_path(&current_input);
    handle_node(
        cur_node,
        &mut current_menu,
        &mut current_input,
        &term,
        &mut out_proxy,
        root,
    )?;

    ctrlc::set_handler(move || {
        _ = Term::stderr().show_cursor();
        exit(1);
    })?;

    loop {
        render_menu(current_menu, &current_input, &mut out_proxy)?;
        debug!("Current input: {current_input:?}");
        let char = term.read_char().context("reading char")?;
        debug!("got char: {char}");
        if char == 127 as char {
            debug!("detected backspace");
            current_input.pop();
        } else {
            current_input.push(char);
        }
        let cur_node = root_node.follow_path(&current_input);
        handle_node(
            cur_node,
            &mut current_menu,
            &mut current_input,
            &term,
            &mut out_proxy,
            root,
        )?;
        term.clear_last_lines(out_proxy.n_lines)?;
        out_proxy.n_lines = 0;
    }
}

fn handle_node<'a>(
    new_node: Option<NodeRef<'a>>,
    current_menu: &mut &'a Menu,
    current_input: &mut Vec<char>,
    term: &Term,
    out_proxy: &mut OutProxy,
    root_menu: &'a Menu,
) -> Result<()> {
    match new_node {
        Some(NodeRef::Command(c)) => return run_command(c, term),
        Some(NodeRef::Menu(m)) => {
            *current_menu = m;
        }
        None => {
            writeln!(out_proxy, "Warning, input argument was invalid")?;
            current_input.clear();
            *current_menu = root_menu;
        }
    }
    Ok(())
}

fn run_command(cmd: &parser::Command, term: &Term) -> Result<()> {
    debug!("Running: {cmd}");
    for var in &cmd.env_vars {
        let val = query_env_var(var).context("querying env var")?;
        // uppon calling exec, the env vars are kept, so just setting them here
        // means setting them for the callee
        env::set_var(var, val);
    }
    term.clear_last_lines(cmd.env_vars.len())
        .context("Clearing input lines")?;

    Err(anyhow!(
        "{:?}",
        exec::execvp("bash", &["bash", "-c", cmd.exec_str.as_str()])
    ))
}

fn query_env_var(name: &str) -> Result<String> {
    Ok(Input::new()
        .with_prompt(format!("Value for {name}"))
        .interact_text()?)
}

fn render_menu(
    current_menu: &Menu,
    _current_input: &[char],
    out_proxy: &mut OutProxy,
) -> Result<()> {
    let keysection_len = current_menu
        .entries
        .iter()
        .map(|(keys, _)| keys.len())
        .max()
        .expect("empty menu")
        + 1;
    for (keys, node) in &current_menu.entries {
        let keys = format!("{}:", String::from_iter(keys));
        let keys = pad_str(&keys, keysection_len, Alignment::Left, None);
        writeln!(out_proxy, "{keys} {node}")?;
    }
    Ok(())
}

impl<'a> NodeRef<'a> {
    pub fn follow_path(self, path: &[char]) -> Option<NodeRef<'a>> {
        match self {
            Self::Menu(this) => match find_submenus_for(this, path) {
                Submenus::Exact(next_node, remaining_path) => next_node.follow_path(remaining_path),
                Submenus::Incomplete => Some(self),
                Submenus::None => None,
            },
            Self::Command(_) => Some(self),
        }
    }
}

pub fn find_submenus_for<'a, 'b>(menu: &'a Menu, path: &'b [char]) -> Submenus<'a, 'b> {
    // The base idea here is to compare the path with valid entries character wise.
    // A vec of options of chars is used, so it can be set to none, if it doesn't match any more
    // If it matches, the first char is removed. If after the removal of the char, the slice is
    // empty, we have an exact match and return it.
    // If we don't have any options left after checking the complete path, that means the path was
    // invalid, otherwise it's not yet complete
    let mut entries: Vec<_> = menu
        .entries
        .iter()
        .map(|(chars, nodes)| (Some(chars.as_slice()), nodes))
        .collect();
    for (i, c) in path.iter().enumerate() {
        for (chars_opt, node) in &mut entries {
            if let Some(chars) = chars_opt {
                // this could panic, but empty menu entries aren't allowed and won't happen.
                // and, since it is immediately checked whether an entry is empty uppon removal,
                // we won't produce that state either
                if chars[0] == *c {
                    *chars = &chars[1..];
                    if chars.len() == 0 {
                        return Submenus::Exact((*node).into(), &path[i + 1..]);
                    }
                } else {
                    *chars_opt = None;
                }
            }
        }
    }

    if entries.iter().all(|(chars, _)| chars.is_none()) {
        Submenus::None
    } else {
        Submenus::Incomplete
    }
}

impl<'a> From<&'a parser::Node> for NodeRef<'a> {
    fn from(value: &'a parser::Node) -> Self {
        match value {
            parser::Node::Menu(m) => Self::Menu(m),
            parser::Node::Command(c) => Self::Command(c),
        }
    }
}
