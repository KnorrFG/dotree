use anyhow::{anyhow, Context, Result};
use console::{pad_str, style, Alignment, Key, Term};
use dialoguer::Input;
use log::debug;
use std::io::Write;
use std::{env, process::exit};

use crate::outproxy::OutProxy;
use crate::parser::{self, Menu, Node};

#[derive(Debug, Clone)]
enum Submenus<'a> {
    Exact(&'a Node, InputBuffer),
    Incomplete(InputBuffer),
    None,
}

#[derive(Debug, Clone)]
struct InputBuffer {
    chars: Vec<char>,
    pos: usize,
}

pub fn run(root_node: &Node, input: Option<&str>) -> Result<()> {
    let Node::Menu(root_menu) = root_node else {
        panic!("root node isn't a menu")
    };
    let mut current_menu = root_menu;
    let current_input_chars = if let Some(input) = input {
        input.chars().collect()
    } else {
        vec![]
    };

    let term = Term::stdout();
    let mut out_proxy = OutProxy::new();

    let mut current_input;
    let (cur_node, current_input_) =
        follow_path(root_node, InputBuffer::from_vec(current_input_chars));
    current_input = current_input_;
    handle_node(
        cur_node,
        &mut current_menu,
        &term,
        &mut out_proxy,
        root_menu,
    )?;

    ctrlc::set_handler(move || {
        _ = Term::stderr().show_cursor();
        exit(1);
    })?;

    loop {
        render_menu(
            current_menu,
            current_input.as_slice().iter().collect::<String>().as_ref(),
            &mut out_proxy,
        )?;
        let mut chars = current_input.take();
        debug!("Current input: {chars:?}");
        let key = term.read_key().context("reading char")?;
        debug!("got char: {key:?}");
        match key {
            Key::Char(c) => {
                chars.push(c);
            }
            Key::Backspace => {
                chars.pop();
            }
            Key::Escape => {
                term.clear_last_lines(out_proxy.n_lines)?;
                return Ok(());
            }
            _ => {}
        }
        let (cur_node, current_input_) = follow_path(root_node, InputBuffer::from_vec(chars));
        current_input = current_input_;
        handle_node(
            cur_node,
            &mut current_menu,
            &term,
            &mut out_proxy,
            root_menu,
        )?;
        term.clear_last_lines(out_proxy.n_lines)?;
        out_proxy.n_lines = 0;
    }
}

fn handle_node<'a>(
    new_node: Option<&'a Node>,
    current_menu: &mut &'a Menu,
    term: &Term,
    out_proxy: &mut OutProxy,
    root_menu: &'a Menu,
) -> Result<()> {
    match new_node {
        Some(Node::Command(c)) => {
            term.clear_last_lines(out_proxy.n_lines)?;
            term.show_cursor()?;
            return run_command(c, term);
        }
        Some(Node::Menu(m)) => {
            *current_menu = m;
        }
        None => {
            writeln!(out_proxy, "Warning, input argument was invalid")?;
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

fn render_menu(current_menu: &Menu, remaining_path: &str, out_proxy: &mut OutProxy) -> Result<()> {
    let keysection_len = current_menu
        .entries
        .keys()
        .map(|keys| keys.len())
        .max()
        .expect("empty menu")
        + 1;
    for (keys, node) in &current_menu.entries {
        let keys = String::from_iter(keys);
        let keys = if let Some(rest) = keys.strip_prefix(remaining_path) {
            format!("{}{}:", style(remaining_path).green().bright().bold(), rest)
        } else {
            format!("{keys}:")
        };
        let keys = pad_str(&keys, keysection_len, Alignment::Left, None);
        writeln!(out_proxy, "{keys} {node}")?;
    }
    Ok(())
}

fn follow_path(node: &Node, buf: InputBuffer) -> (Option<&Node>, InputBuffer) {
    match node {
        Node::Menu(this) => match find_submenus_for(this, buf) {
            Submenus::Exact(next_node, buf) => follow_path(next_node, buf),
            Submenus::Incomplete(buf) => (Some(node), buf),
            Submenus::None => (
                None,
                InputBuffer {
                    chars: vec![],
                    pos: 0,
                },
            ),
        },
        Node::Command(_) => (Some(node), buf),
    }
}

fn find_submenus_for(menu: &Menu, buf: InputBuffer) -> Submenus {
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
    for (i, c) in buf.as_slice().iter().enumerate() {
        for (chars_opt, node) in &mut entries {
            if let Some(chars) = chars_opt {
                // this could panic, but empty menu entries aren't allowed and won't happen.
                // and, since it is immediately checked whether an entry is empty uppon removal,
                // we won't produce that state either
                if chars[0] == *c {
                    *chars = &chars[1..];
                    if chars.is_empty() {
                        return Submenus::Exact(node, buf.with_offset(i + 1));
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
        Submenus::Incomplete(buf)
    }
}

impl InputBuffer {
    fn take(self) -> Vec<char> {
        self.chars
    }

    fn from_vec(chars: Vec<char>) -> Self {
        InputBuffer { chars, pos: 0 }
    }

    fn as_slice(&self) -> &[char] {
        &self.chars[self.pos..]
    }

    fn with_offset(mut self, offset: usize) -> Self {
        self.pos += offset;
        self
    }
}
