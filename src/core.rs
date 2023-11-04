use anyhow::{anyhow, bail, ensure, Context, Result};
use console::{pad_str, style, Alignment, Key, Term};
use log::debug;
use rustyline::completion::{Completer, FilenameCompleter};
use rustyline::config;
use rustyline::highlight::Highlighter;
use rustyline::{Completer, Helper, Hinter, Validator};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::{env, process::exit};
use std::{fs, io};

use crate::outproxy::OutProxy;
use crate::parser::{self, Menu, Node};
use crate::rt_conf;

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

pub fn run(root_node: &Node, input: &[String]) -> Result<()> {
    let Node::Menu(root_menu) = root_node else {
        panic!("root node isn't a menu")
    };
    let mut current_menu = root_menu;
    let current_input_chars = if let Some(input) = input.first() {
        input.chars().collect()
    } else {
        vec![]
    };
    let arg_vals = if input.len() > 1 { &input[1..] } else { &[] };

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
        arg_vals,
    )?;

    ctrlc::set_handler(move || {
        _ = Term::stderr().show_cursor();
    })?;

    loop {
        render_menu(
            current_menu,
            current_input.as_slice().iter().collect::<String>().as_ref(),
            &mut out_proxy,
        )?;
        let mut chars = current_input.take();
        debug!("Current input: {chars:?}");

        let key = match term.read_key() {
            Ok(k) => k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                term.clear_last_lines(out_proxy.n_lines)?;
                return Ok(());
            }
            Err(e) => {
                bail!("Error while waiting for key: {e:?}");
            }
        };

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
            arg_vals,
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
    arg_vals: &[String],
) -> Result<()> {
    match new_node {
        Some(Node::Command(c)) => {
            term.clear_last_lines(out_proxy.n_lines)?;
            term.show_cursor()?;
            return run_command(c, term, arg_vals);
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

fn run_command(cmd: &parser::Command, term: &Term, arg_vals: &[String]) -> Result<()> {
    let mut history = load_hist().context("loading hist")?;
    debug!("Running: {cmd}");

    ensure!(
        arg_vals.len() <= cmd.env_vars.len(),
        "Too many arguments for this command"
    );

    if let Some(wd) = rt_conf::local_conf_dir() {
        env::set_current_dir(wd).context("Changing working directory")?;
    }

    for i in 0..cmd.env_vars.len() {
        let var = &cmd.env_vars[i];
        let val = if let Some(val) = arg_vals.get(i) {
            val
        } else {
            history = query_env_var(var, history).context("querying env var")?;
            history.last().unwrap()
        };
        // uppon calling exec, the env vars are kept, so just setting them here
        // means setting them for the callee
        env::set_var(var, val);
    }
    term.clear_last_lines(cmd.env_vars.len())
        .context("Clearing input lines")?;
    store_hist(history).context("Storing history")?;

    Err(anyhow!(
        "{:?}",
        exec::execvp("bash", &["bash", "-c", cmd.exec_str.as_str()])
    ))
}

fn get_hist_path() -> Result<PathBuf> {
    let dir = if let Some(sd) = dirs::state_dir() {
        sd
    } else {
        dirs::data_local_dir().ok_or(anyhow!("couldn't get local dir"))?
    };
    Ok(dir.join("dthist"))
}

fn load_hist() -> Result<Vec<String>> {
    let hist_path = get_hist_path()?;
    Ok(if hist_path.exists() {
        fs::read_to_string(hist_path)
            .context("reading file")?
            .lines()
            .map(|x| x.to_string())
            .collect()
    } else {
        vec![]
    })
}

fn store_hist(hist: Vec<String>) -> Result<()> {
    #[cfg(windows)]
    let line_ending = "\r\n";
    #[cfg(not(windows))]
    let line_ending = "\n";

    fs::write(get_hist_path()?, hist.join(line_ending))?;
    Ok(())
}

#[derive(Helper, Completer, Hinter, Validator)]
struct RlHelper {
    #[rustyline(Completer)]
    completer: FilenameCompleter,
}
impl Highlighter for RlHelper {}

fn query_env_var(name: &str, mut hist: Vec<String>) -> Result<Vec<String>> {
    let mut rl = rustyline::Editor::new()?;
    rl.set_helper(Some(RlHelper {
        completer: FilenameCompleter::new(),
    }));
    for h in &hist {
        rl.add_history_entry(h)?;
    }
    let line = rl.readline(&format!("Value for {name}: "))?;
    hist.push(line);
    Ok(hist)
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
