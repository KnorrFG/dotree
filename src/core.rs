use anyhow::{anyhow, Context, Result};
use console::Term;
use dialoguer::Input;
use log::debug;
use std::{
    collections::{HashMap, VecDeque},
    env,
    process::exit,
};

#[derive(Debug)]
pub enum Node {
    Menu(Menu),
    Command(Command),
}

impl std::fmt::Display for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Node::Menu(m) => write!(f, "{}", m.name),
            Node::Command(c) => write!(f, "{c}"),
        }
    }
}

#[derive(Debug)]
pub struct Menu {
    pub name: String,
    pub entries: HashMap<String, Node>,
}

#[derive(Debug)]
pub struct Command {
    pub exec_str: String,
    pub name: Option<String>,
    pub env_vars: Vec<String>,
}

impl std::fmt::Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let v = if let Some(name) = &self.name {
            name
        } else {
            &self.exec_str
        };
        write!(f, "{v}")
    }
}

pub fn run(root: &Menu, input: Option<&str>) -> Result<()> {
    let mut current_menu = root;
    let mut current_input = vec![];
    let term = Term::stdout();
    let mut written_lines = 0;

    use ProcessOutput::*;
    if let Some(input) = input {
        for c in input.chars() {
            current_input.push(c);
            match process_input(current_menu, &current_input) {
                Pending => {}
                Invalid => {
                    current_input.clear();
                    written_lines += print_invalid_arg_warning();
                    break;
                }
                NextMenu(m) => {
                    current_input.clear();
                    current_menu = m;
                }
                Command(c) => {
                    term.show_cursor().context("showing cursor")?;
                    term.flush().context("flushing term")?;
                    return run_command(c, term);
                }
            }
        }
    }
    ctrlc::set_handler(move || {
        _ = Term::stderr().show_cursor();
        exit(1);
    })?;

    loop {
        written_lines += render_menu(current_menu, &current_input)?;
        debug!("Current input: {current_input:?}");
        let char = term.read_char().context("reading char")?;
        debug!("got char: {char}");
        if char == 127 as char {
            debug!("detected backspace");
            current_input.pop();
        } else {
            current_input.push(char);
        }
        match process_input(current_menu, &current_input) {
            Pending => {}
            Invalid => {
                written_lines += print_error_msg();
                current_input.clear();
            }
            NextMenu(m) => {
                current_input.clear();
                current_menu = m;
            }
            Command(c) => {
                term.clear_last_lines(written_lines)?;
                term.show_cursor().context("showing cursor")?;
                return run_command(c, term);
            }
        }
        term.clear_last_lines(written_lines)?;
        written_lines = 0;
    }
}

fn print_invalid_arg_warning() -> usize {
    eprintln!("Warning, input argument was invalid");
    1
}

fn run_command(cmd: &Command, term: Term) -> Result<()> {
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

fn render_menu(current_menu: &Menu, _current_input: &[char]) -> Result<usize> {
    for (keys, node) in &current_menu.entries {
        println!("{keys}: {node}");
    }
    Ok(current_menu.entries.len())
}

fn print_error_msg() -> usize {
    0
}

fn process_input<'a>(current_menu: &'a Menu, current_input: &[char]) -> ProcessOutput<'a> {
    let mut sub_menus: Vec<Option<(VecDeque<char>, &Node)>> = current_menu
        .entries
        .iter()
        .map(|(keys, node)| {
            Some((
                VecDeque::<char>::from(keys.chars().collect::<Vec<_>>()),
                node,
            ))
        })
        .collect();

    for c in current_input {
        for entry in &mut sub_menus {
            if let Some((keys, _)) = entry {
                if keys.pop_front() != Some(*c) {
                    *entry = None;
                }
            }
        }
    }

    let remaining_entries: Vec<_> = sub_menus.into_iter().flatten().collect();
    debug!("remaining entries: {remaining_entries:?}");
    use ProcessOutput::*;
    match remaining_entries.len() {
        0 => Invalid,
        1 => match remaining_entries[0].1 {
            Node::Menu(m) => NextMenu(m),
            Node::Command(c) => Command(c),
        },
        _ => Pending,
    }
}

enum ProcessOutput<'a> {
    Pending,
    Invalid,
    NextMenu(&'a Menu),
    Command(&'a Command),
}
