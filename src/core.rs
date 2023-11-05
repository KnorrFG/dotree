use anyhow::{anyhow, bail, ensure, Context, Result};
use console::{pad_str, style, Alignment, Key, Term};
use log::debug;
use rustyline::completion::FilenameCompleter;
use rustyline::highlight::Highlighter;
use rustyline::{Completer, Helper, Hinter, Validator};
use std::env;
use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;
use std::{fs, io};

use crate::outproxy::OutProxy;
use crate::parser::{self, CommandSetting, Menu, Node};
use crate::rt_conf;

#[derive(Debug, Clone)]
enum Submenus<'a> {
    Exact(&'a Node, usize),
    Incomplete(usize),
    None,
}

pub fn run(root_node: &Node, input: &[String]) -> Result<()> {
    let mut input_chars = if let Some(input) = input.first() {
        input.chars().collect()
    } else {
        vec![]
    };
    let arg_vals = if input.len() > 1 { &input[1..] } else { &[] };

    let term = Term::stdout();
    let mut out_proxy = OutProxy::new();
    let (found_node, input_offset) = follow_path(root_node, &input_chars, 0);
    let mut input_pos = input_offset;
    let mut current_node = if let Some(found_node) = found_node {
        found_node
    } else {
        input_chars.clear();
        root_node
    };

    // we need to create a handler, because, if we don't the program will terminate abnormally
    // but if we do, readline will return an io::Error with kind Interrupted, when ctrl+c
    // is pressed
    ctrlc::set_handler(|| {})?;

    loop {
        match current_node {
            Node::Command(c) => {
                if c.repeat() {
                    input_chars.pop();
                }
                run_command(c, &term, arg_vals)?;
            }
            Node::Menu(m) => {
                term.clear_last_lines(out_proxy.n_lines)?;
                out_proxy.n_lines = 0;
                render_menu(m, &input_chars[input_pos..], &mut out_proxy)?;
            }
        }

        // returns true when the user pressed Esc or Ctrl+c, which means we should exit
        if get_input(&mut input_chars, &term)? {
            term.clear_last_lines(out_proxy.n_lines)?;
            Term::stderr().show_cursor()?;
            break Ok(());
        };

        let (found_node, input_offset_) = follow_path(root_node, &input_chars, 0);
        input_pos = input_offset_;
        current_node = if let Some(found_node) = found_node {
            found_node
        } else {
            input_chars.clear();
            root_node
        };
    }
}

type Exit = bool;
fn get_input(input_chars: &mut Vec<char>, term: &Term) -> Result<Exit> {
    let key = match term.read_key() {
        Ok(k) => k,
        Err(e) if e.kind() == io::ErrorKind::Interrupted => {
            return Ok(true);
        }
        Err(e) => {
            bail!("Error while waiting for key: {e:?}");
        }
    };

    debug!("got char: {key:?}");
    match key {
        Key::Char(c) => {
            input_chars.push(c);
        }
        Key::Backspace => {
            input_chars.pop();
        }
        Key::Escape => {
            return Ok(true);
        }
        _ => {}
    }
    Ok(false)
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
    term.clear_last_lines(cmd.env_vars.len() - arg_vals.len())
        .context("Clearing input lines")?;
    store_hist(history).context("Storing history")?;

    let shell = rt_conf::shell_def();
    debug!("shell: {shell:?}");
    let mut args = shell.args_with(cmd.exec_str.as_str());
    if cmd.settings.contains(&CommandSetting::Repeat) {
        run_subcommand(
            &shell.name,
            &args,
            cmd.settings.contains(&CommandSetting::IgnoreResult),
        )
    } else {
        args.insert(0, &shell.name);
        Err(anyhow!(
            "error executing command: \n{:?}",
            exec::execvp(&shell.name, &args)
        ))
    }
}

fn run_subcommand(prog: &str, args: &[&str], ignore_result: bool) -> Result<()> {
    let status = std::process::Command::new(prog)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .args(args)
        .status()?;
    if !ignore_result && !status.success() {
        Err(anyhow!("Process didn't exit successfully: {status:?}"))
    } else {
        Ok(())
    }
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

fn render_menu(
    current_menu: &Menu,
    remaining_path: &[char],
    out_proxy: &mut OutProxy,
) -> Result<()> {
    let remaining_path = String::from_iter(remaining_path);
    let keysection_len = current_menu
        .entries
        .keys()
        .map(|keys| keys.len())
        .max()
        .expect("empty menu")
        + 1;
    for (keys, node) in &current_menu.entries {
        let keys = String::from_iter(keys);
        let keys = if let Some(rest) = keys.strip_prefix(&remaining_path) {
            format!(
                "{}{}:",
                style(&remaining_path).green().bright().bold(),
                rest
            )
        } else {
            format!("{keys}:")
        };
        let keys = pad_str(&keys, keysection_len, Alignment::Left, None);
        writeln!(out_proxy, "{keys} {node}")?;
    }
    Ok(())
}

fn follow_path<'a>(node: &'a Node, input_chars: &[char], pos: usize) -> (Option<&'a Node>, usize) {
    match node {
        Node::Menu(this) => match find_submenus_for(this, input_chars, pos) {
            Submenus::Exact(next_node, new_pos) => follow_path(next_node, input_chars, new_pos),
            Submenus::Incomplete(new_pos) => (Some(node), new_pos),
            Submenus::None => (None, 0),
        },
        Node::Command(_) => (Some(node), pos),
    }
}

fn find_submenus_for<'a>(menu: &'a Menu, input_chars: &[char], pos: usize) -> Submenus<'a> {
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
    for (i, c) in input_chars[pos..].iter().enumerate() {
        for (chars_opt, node) in &mut entries {
            if let Some(chars) = chars_opt {
                // this could panic, but empty menu entries aren't allowed and won't happen.
                // and, since it is immediately checked whether an entry is empty uppon removal,
                // we won't produce that state either
                if chars[0] == *c {
                    *chars = &chars[1..];
                    if chars.is_empty() {
                        return Submenus::Exact(node, pos + i + 1);
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
        Submenus::Incomplete(pos)
    }
}
