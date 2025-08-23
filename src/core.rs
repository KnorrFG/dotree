use anyhow::{anyhow, bail, ensure, Context, Result};
use console::{pad_str, style, Alignment, Key, Term};
use log::debug;

use miniserde::{json, Deserialize, Serialize};
use rustyline::{
    completion::FilenameCompleter, highlight::Highlighter, Completer, Helper, Hinter, Validator,
};

use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Stdio,
};

use crate::{
    outproxy::OutProxy,
    parser::{self, CommandSetting, Menu, Node, SnippetTable},
    rt_conf,
};

#[derive(Debug, Clone, Copy)]
enum MenuMatch<'a> {
    Exact(&'a Node, usize),
    Incomplete(&'a Menu, usize),
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfiguredCommand {
    shell_name: String,
    cmd_repeats: bool,
    cmd_ignores_result: bool,
    cmd_toggle_echo_setting: bool,
    args: Vec<String>,
}

pub struct Runner<'a> {
    input_chars: Vec<char>,
    out_proxy: OutProxy,
    input_pos: usize,
    current_menu: &'a Menu,
    root_menu: &'a Menu,
    snippet_table: &'a SnippetTable,
    arg_vals: &'a [String],
    term: Term,
}

impl<'a> Runner<'a> {
    pub fn new(
        root_menu: &'a Menu,
        input: &'a [String],
        snippet_table: &'a SnippetTable,
    ) -> Result<(Self, Option<ConfiguredCommand>)> {
        let input_chars = if let Some(input) = input.first() {
            input.chars().collect()
        } else {
            vec![]
        };
        let arg_vals = if input.len() > 1 { &input[1..] } else { &[] };

        let out_proxy = OutProxy::new();
        let mut this = Self {
            input_chars,
            out_proxy,
            input_pos: 0,
            current_menu: root_menu,
            root_menu,
            snippet_table,
            arg_vals,
            term: Term::stdout(),
        };
        let command = this.resolve_input()?;
        ctrlc::set_handler(|| {})?;

        Ok((this, command))
    }

    /// renders, takes an input and then resolves the input.
    /// if it resolves to a command it's configured and returned,
    /// otherwise this just loops, unless ctrl + c is pressed
    pub fn run(&mut self) -> Result<Option<ConfiguredCommand>> {
        loop {
            self.clear_output()?;
            render_menu(
                self.current_menu,
                &self.input_chars[self.input_pos..],
                &mut self.out_proxy,
            )?;

            // returns true when the user pressed Esc or Ctrl+c, which means we should exit
            if get_input(&mut self.input_chars, &self.term)? {
                break Ok(None);
            };

            let mb_cmd = self.resolve_input()?;
            if mb_cmd.is_some() {
                break Ok(mb_cmd);
            }
        }
    }

    pub fn clear_output(&mut self) -> Result<(), anyhow::Error> {
        self.term.clear_last_lines(self.out_proxy.n_lines)?;
        self.out_proxy.n_lines = 0;
        Ok(())
    }

    /// sets the input_pos and the current_menu according to the input.
    /// returns a configured command if the input resolves to one
    pub fn resolve_input(&mut self) -> Result<Option<ConfiguredCommand>> {
        debug!("Follow path");
        match follow_path(self.root_menu, &self.input_chars, 0) {
            MenuMatch::Exact(node, offset) => {
                self.input_pos = offset;
                match node {
                    Node::Menu(menu) => {
                        debug!("Updating menu to: {menu}");
                        self.current_menu = menu;
                    }
                    Node::Command(command) => {
                        debug!("found command: {command:?}");
                        if command.repeat() {
                            self.input_chars.pop();
                            self.input_pos -= 1;
                        }
                        return Ok(Some(configure_command(
                            command,
                            &self.term,
                            self.arg_vals,
                            self.snippet_table,
                        )?));
                    }
                }
            }
            MenuMatch::Incomplete(menu, offset) => {
                debug!("Incomplete match, pos = {offset}");
                self.current_menu = menu;
                self.input_pos = offset;
            }
            MenuMatch::None => {
                debug!("missmatch");
                self.input_chars.clear();
                self.current_menu = self.root_menu;
            }
        }
        Ok(None)
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

fn configure_command(
    cmd: &parser::Command,
    term: &Term,
    arg_vals: &[String],
    snippet_table: &SnippetTable,
) -> Result<ConfiguredCommand> {
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
            history = query_env_var(&var.name, &var.value, history).context("querying env var")?;
            history.last().unwrap()
        };
        // uppon calling exec, the env vars are kept, so just setting them here
        // means setting them for the callee
        env::set_var(&var.name, val);
    }
    term.clear_last_lines(cmd.env_vars.len() - arg_vals.len())
        .context("Clearing input lines")?;
    store_hist(history).context("Storing history")?;

    let shell = cmd
        .shell
        .clone()
        .or_else(|| rt_conf::settings().shell_def.clone())
        .unwrap_or_default();
    debug!("shell: {shell:?}");
    let arg = cmd
        .exec_str
        .resolve(snippet_table)
        .context(format!("resolving {}", cmd.exec_str))?;
    let args = shell.args_with(arg.as_str());
    // todo: Ich wuerde sagen an dieser stelle werden shell und args, und vermutlich cmd
    // weggespeichert, sodass man sie dann wieder laden kann
    // man muesste außerdem die elemtente von cmd speichern, die gebraucht werden.
    // die folge waere, dass aenderungen am config file dann komplett ignoriert werden.
    // die einfache loesung waere den cache zu loeschen, wenn das config file geupdated
    // wurde. Bzw den cache zu invalidieren wenn das edit date der config neuer ist
    // als das des caches.
    // Wenn ich unterschiedliche caches fuer local und global mode will, sollte
    // ich das speichern vermutlich nicht hier tun, und nur die zu speichernde struktur
    // zurueck schicken. In dem fall, kann ich kein exec mehr verwenden.
    // alternativ koennte ich auch die info wo gespeichert werden soll bis hier
    // durchschleifen, aber das scheint mir auch nicht sinnvoll
    // vermutlich sollte man alles was in dieser funktion passiert bis hier
    // in eine eigene funktion verlagern deren result kann man dann
    // wegspeichern, und dann hier rein schmeißen
    let conf = ConfiguredCommand {
        shell_name: shell.name.to_string(),
        cmd_repeats: cmd.settings.contains(&CommandSetting::Repeat),
        cmd_ignores_result: cmd.settings.contains(&CommandSetting::IgnoreResult),
        cmd_toggle_echo_setting: cmd.toggle_echo_setting,
        args: args.iter().map(|x| x.to_string()).collect(),
    };
    Ok(conf)
}

impl ConfiguredCommand {
    pub fn repeats(&self) -> bool {
        self.cmd_repeats
    }

    pub fn run(&self) -> Result<()> {
        if !self.cmd_repeats && self.cmd_toggle_echo_setting != rt_conf::settings().echo_by_default
        {
            eprintln!("{}", self.args.iter().last().unwrap());
        }

        let mut cmd = std::process::Command::new(&self.shell_name);
        cmd.args(&self.args);
        if self.cmd_repeats {
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }

        let status = cmd.status()?;
        if !self.cmd_ignores_result && !status.success() {
            Err(anyhow!("Process didn't exit successfully: {status:?}"))
        } else {
            Ok(())
        }
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

fn get_last_command_cache_path(local_conf_dir: Option<&Path>) -> Result<PathBuf> {
    if let Some(local_dir) = local_conf_dir {
        Ok(local_dir.join(".dtcache"))
    } else {
        let dir = if let Some(sd) = dirs::state_dir() {
            sd
        } else {
            dirs::data_local_dir().ok_or(anyhow!("couldn't get local dir"))?
        };
        Ok(dir.join("dtcache"))
    }
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

pub fn store_command(cmd: &ConfiguredCommand, local_conf_dir: Option<&Path>) -> Result<()> {
    let last_command_cache_path = get_last_command_cache_path(local_conf_dir)?;
    fs::write(last_command_cache_path, json::to_string(cmd))?;
    Ok(())
}

pub fn load_command(local_conf_dir: Option<&Path>) -> Result<Option<ConfiguredCommand>> {
    let last_command_cache_path = get_last_command_cache_path(local_conf_dir)?;
    if last_command_cache_path.exists() {
        let src = fs::read_to_string(last_command_cache_path)?;
        Ok(json::from_str(&src)?)
    } else {
        Ok(None)
    }
}

fn store_hist(hist: Vec<String>) -> Result<()> {
    #[cfg(windows)]
    let line_ending = "\r\n";
    #[cfg(not(windows))]
    let line_ending = "\n";
    let hist_path = get_hist_path()?;
    fs::create_dir_all(hist_path.parent().context("Getting history file dir")?)
        .context("creating history file dir")?;
    fs::write(hist_path, hist.join(line_ending))?;
    Ok(())
}

#[derive(Helper, Completer, Hinter, Validator)]
struct RlHelper {
    #[rustyline(Completer)]
    completer: FilenameCompleter,
}
impl Highlighter for RlHelper {}

fn query_env_var(
    name: &str,
    default_val: &Option<String>,
    mut hist: Vec<String>,
) -> Result<Vec<String>> {
    let mut rl = rustyline::Editor::new()?;
    rl.set_helper(Some(RlHelper {
        completer: FilenameCompleter::new(),
    }));
    for h in &hist {
        rl.add_history_entry(h)?;
    }
    let line = rl.readline(&format!(
        "Value for {name}{default}: ",
        default = if let Some(default_val) = default_val {
            format!(" ({default_val})")
        } else {
            String::new()
        }
    ))?;

    if line.is_empty() {
        if let Some(default_val) = default_val {
            hist.push(default_val.to_string());
            return Ok(hist);
        }
    }
    hist.push(line);
    Ok(hist)
}

fn render_menu(
    current_menu: &Menu,
    remaining_path: &[char],
    out_proxy: &mut OutProxy,
) -> Result<()> {
    debug!("Rendering menu: {current_menu}");
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

fn follow_path<'a>(menu: &'a Menu, input_chars: &[char], pos: usize) -> MenuMatch<'a> {
    match find_menu_entry(menu, input_chars, pos) {
        menu_match @ MenuMatch::Exact(Node::Menu(menu), new_pos) => {
            debug!("Follow path exact match, input_chars: {input_chars:?}, new_pow: {new_pos}");
            if new_pos == input_chars.len() {
                menu_match
            } else {
                follow_path(menu, input_chars, new_pos)
            }
        }
        res => res,
    }
}

fn find_menu_entry<'a>(menu: &'a Menu, input_chars: &[char], pos: usize) -> MenuMatch<'a> {
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
                        return MenuMatch::Exact(node, pos + i + 1);
                    }
                } else {
                    *chars_opt = None;
                }
            }
        }
    }

    if entries.iter().all(|(chars, _)| chars.is_none()) {
        MenuMatch::None
    } else {
        MenuMatch::Incomplete(menu, pos)
    }
}
