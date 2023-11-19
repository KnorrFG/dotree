use hashbrown::HashMap;
use log::debug;
use std::collections::VecDeque;

use pest::{
    iterators::{Pair, Pairs},
    Parser,
};
use pest_derive::Parser;

use anyhow::{anyhow, ensure, Context, Result};

#[derive(Parser)]
#[grammar = "grammar.pest"]
struct ConfigParser;

#[derive(Debug, Clone)]
pub enum Node {
    Menu(Menu),
    Command(Command),
}

#[derive(Debug, Clone)]
pub struct Menu {
    pub name: String,
    pub display_name: Option<String>,
    pub entries: HashMap<Vec<char>, Node>,
}

#[derive(Debug, Clone)]
pub struct Command {
    pub exec_str: StringExpr,
    pub settings: Vec<CommandSetting>,
    pub name: Option<String>,
    pub shell: Option<ShellDef>,
    pub env_vars: Vec<VarDef>,
    pub toggle_echo_setting: bool,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum CommandSetting {
    Repeat,
    IgnoreResult,
}

#[derive(Debug, Clone)]
pub struct ShellDef {
    pub name: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct VarDef {
    pub name: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone)]
struct RawMenu<'a> {
    display_name: Option<String>,
    body: Pairs<'a, Rule>,
}

#[derive(Debug, Clone)]
pub enum StringExprElem {
    Symbol(String),
    String(String),
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub shell_def: Option<ShellDef>,
    pub echo_by_default: bool,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub menu: Menu,
    pub settings: Settings,
    pub snippet_table: SnippetTable,
}

#[derive(Debug, Clone)]
pub struct StringExpr(Vec<StringExprElem>);

pub type SnippetTable = HashMap<String, StringExpr>;

trait INext: Sized {
    fn inext(self) -> Self;
    fn nnext(mut self, n: usize) -> Self {
        for _ in 0..n {
            self = self.inext();
        }
        self
    }
}

impl INext for Pair<'_, Rule> {
    fn inext(self) -> Self {
        self.into_inner().next().unwrap()
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            shell_def: None,
            echo_by_default: true,
        }
    }
}

fn from_string(p: Pair<'_, Rule>) -> String {
    p.nnext(2).as_str().to_string()
}

pub fn parse(src: &str) -> Result<Config> {
    let mut pairs = ConfigParser::parse(Rule::file, src).context("Parsing source")?;
    let file = pairs.next().unwrap();
    assert!(file.as_rule() == Rule::file);

    let (settings, entries) = parse_settings(file.into_inner());

    let menus = get_menu_table(entries.clone());
    let snippet_table = get_snippet_table(entries);
    let menu = parse_menu("root", &menus)?;

    Ok(Config {
        menu,
        settings,
        snippet_table,
    })
}

fn parse_settings(mut entries: Pairs<Rule>) -> (Settings, Pairs<Rule>) {
    let mut res = Settings::default();
    debug!("Parsing settings: \n{entries:?}");
    while let Some(first_entry) = entries.peek() {
        if first_entry.as_rule() != Rule::setting {
            break;
        }
        let first_entry = first_entry.inext();
        match first_entry.as_rule() {
            Rule::shell_def => {
                res.shell_def = Some(parse_shell_def(first_entry));
                debug!("parsing shell_def result: {:?}", res.shell_def);
            }
            Rule::echo_setting => {
                res.echo_by_default = parse_echo_setting(first_entry);
                debug!("parsing echo_setting result: {:?}", res.echo_by_default);
            }
            _ => {
                panic!("unexpected rule:\n{first_entry:#?}");
            }
        }
        _ = entries.next();
    }
    (res, entries)
}

fn get_snippet_table(entries: Pairs<'_, Rule>) -> HashMap<String, StringExpr> {
    let mut res = HashMap::new();
    for e in entries {
        if e.as_rule() == Rule::snippet {
            let mut e = e.into_inner();
            res.insert(
                e.next().unwrap().as_str().to_string(),
                parse_string_expr(e.next().unwrap()),
            );
        }
    }
    res
}

pub fn parse_shell_string(src: &str) -> Result<ShellDef> {
    let mut pairs = ConfigParser::parse(Rule::shell_def, src).context("Parsing shell def")?;
    Ok(parse_shell_def(pairs.next().unwrap()))
}

fn parse_shell_def(p: Pair<'_, Rule>) -> ShellDef {
    let mut elems = VecDeque::new();
    for p in p.into_inner() {
        match p.as_rule() {
            Rule::word => elems.push_back(p.as_str().to_string()),

            Rule::string => elems.push_back(from_string(p)),
            _ => panic!("unexpected rule: {p:?}"),
        }
    }
    ShellDef {
        name: elems.pop_front().unwrap(),
        args: elems.into_iter().collect(),
    }
}

fn get_menu_table(pairs: Pairs<'_, Rule>) -> HashMap<&str, RawMenu<'_>> {
    pairs
        .into_iter()
        .filter(|x| x.as_rule() == Rule::menu)
        .map(|menu| {
            let mut menu_elems = menu.into_inner();
            let first_child = menu_elems.next().unwrap();
            let (display_name, menu_name) = if first_child.as_rule() == Rule::string {
                (Some(from_string(first_child)), menu_elems.next().unwrap())
            } else {
                (None, first_child)
            };
            (
                menu_name.as_str(),
                RawMenu {
                    display_name,
                    body: menu_elems.next().unwrap().into_inner(),
                },
            )
        })
        .collect()
}

fn parse_menu(name: &str, menus: &HashMap<&str, RawMenu<'_>>) -> Result<Menu> {
    let mut entries = HashMap::new();
    let RawMenu { display_name, body } = menus
        .get(name)
        .ok_or(anyhow!("Undefined symbol: {name}"))?
        .clone();
    for entry in body {
        let mut children = entry.into_inner();
        let keys = children.next().unwrap().as_str().chars().collect();
        let child_pair = children.next().unwrap();
        let next_node = match child_pair.as_rule() {
            Rule::symbol => {
                let submenu_name = child_pair.as_str();
                Node::Menu(
                    parse_menu(submenu_name, menus)
                        .context(format!("Parsing submenu: {submenu_name}"))?,
                )
            }
            Rule::quick_command => {
                let (display_name, toggle_echo_setting, exec_str) = parse_quick_command(child_pair);
                Node::Command(Command {
                    exec_str,
                    name: display_name,
                    settings: vec![],
                    env_vars: vec![],
                    shell: None,
                    toggle_echo_setting,
                })
            }
            Rule::anon_command => Node::Command(parse_anon_command(child_pair)),
            _ => {
                panic!("unexpected rule: {child_pair:?}")
            }
        };
        entries.insert(keys, next_node);
    }
    Ok(Menu {
        name: name.to_string(),
        display_name,
        entries,
    })
}

fn parse_anon_command(p: Pair<'_, Rule>) -> Command {
    let body = p.inext();
    let mut elems = body.into_inner();
    let mut parser = CmdBodyParser::default();
    loop {
        let p = elems.next().unwrap();
        if let Some(cmd) = parser.parse(p) {
            break cmd;
        }
    }
}

#[derive(Default)]
struct CmdBodyParser {
    settings: Option<Vec<CommandSetting>>,
    vars: Option<Vec<VarDef>>,
    shell_def: Option<ShellDef>,
}

impl CmdBodyParser {
    fn parse(&mut self, p: Pair<'_, Rule>) -> Option<Command> {
        match p.as_rule() {
            Rule::cmd_settings => {
                self.settings = Some(parse_cmd_settings(p));
                None
            }
            Rule::vars_def => {
                self.vars = Some(parse_vars_def(p));
                None
            }
            Rule::shell_def => {
                self.shell_def = Some(parse_shell_def(p));
                None
            }
            Rule::quick_command => {
                let (display_name, toggle_echo_setting, exec_str) = parse_quick_command(p);
                Some(Command {
                    exec_str,
                    settings: self.settings.take().unwrap_or_default(),
                    name: display_name,
                    env_vars: self.vars.take().unwrap_or_default(),
                    shell: self.shell_def.take(),
                    toggle_echo_setting,
                })
            }
            _ => panic!("unexpected rule: {p:#?}"),
        }
    }
}

fn parse_cmd_settings(p: Pair<'_, Rule>) -> Vec<CommandSetting> {
    let mut res = vec![];
    for pair in p.into_inner() {
        assert!(pair.as_rule() == Rule::symbol);
        res.push(match pair.as_str() {
            "repeat" => CommandSetting::Repeat,
            "ignore_result" => CommandSetting::IgnoreResult,
            other => panic!("invalid command setting: {other}"),
        })
    }
    res
}

fn parse_vars_def(p: Pair<'_, Rule>) -> Vec<VarDef> {
    fn parse_var_def(p: Pair<'_, Rule>) -> VarDef {
        assert!(p.as_rule() == Rule::var_def, "unexpected rule: {p:#?}");
        let mut p = p.into_inner();
        let name_def = p.next().unwrap();
        let value_def = p.next();

        let name = name_def.as_str().to_string();
        let value = value_def.map(|v| {
            assert!(v.as_rule() == Rule::default_var, "unexpected rule: {p:#?}");
            from_string(v.inext())
        });

        VarDef { name, value }
    }

    assert!(p.as_rule() == Rule::vars_def);
    p.into_inner().map(parse_var_def).collect()
}

fn parse_quick_command(pair: Pair<'_, Rule>) -> (Option<String>, bool, StringExpr) {
    assert!(pair.as_rule() == Rule::quick_command);
    let mut name = None;
    let mut toggle_echo = false;
    let mut str_expr = None;

    for elem in pair.into_inner() {
        match elem.as_rule() {
            Rule::command_name => name = Some(from_string(elem.inext())),
            Rule::ECHO_TOGGLE_TOKEN => toggle_echo = true,
            Rule::string_expr => str_expr = Some(parse_string_expr(elem)),
            _ => panic!("unexpected pair: {elem:#?}"),
        }
    }
    (name, toggle_echo, str_expr.unwrap())
}

fn parse_string_expr(p: Pair<'_, Rule>) -> StringExpr {
    let mut res = vec![];
    for e in p.into_inner() {
        assert!(e.as_rule() == Rule::string_expr_elem);
        let actual_elem = e.inext();
        match actual_elem.as_rule() {
            Rule::string => res.push(StringExprElem::String(from_string(actual_elem))),
            Rule::snippet_symbol => res.push(StringExprElem::Symbol(
                actual_elem.as_str()[1..].to_string(),
            )),
            _ => panic!("unexpected symbol"),
        }
    }
    StringExpr(res)
}

fn parse_echo_setting(p: Pair<'_, Rule>) -> bool {
    assert!(p.as_rule() == Rule::echo_setting);
    p.inext().as_str() == "on"
}

impl std::fmt::Display for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Menu(m) => write!(f, "{}", m.display_name.as_ref().unwrap_or(&m.name)),
            Self::Command(c) => write!(f, "{c}"),
        }
    }
}

impl std::fmt::Display for StringExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let elems: Vec<_> = self
            .0
            .iter()
            .map(|x| match x {
                StringExprElem::Symbol(s) => s.clone(),
                StringExprElem::String(s) => format!("{s:?}"),
            })
            .collect();
        write!(f, "{}", elems.join(" + "))
    }
}

impl std::fmt::Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(name) = &self.name {
            write!(f, "{name}")
        } else {
            write!(f, "{}", self.exec_str)
        }
    }
}

impl Command {
    pub fn repeat(&self) -> bool {
        self.settings.contains(&CommandSetting::Repeat)
    }
}

impl Default for ShellDef {
    fn default() -> Self {
        #[cfg(not(windows))]
        let res = ShellDef {
            name: "bash".into(),
            args: vec!["-euo".into(), "pipefail".into(), "-c".into()],
        };

        #[cfg(windows)]
        let res = ShellDef {
            name: "cmd".into(),
            args: vec!["/c".into()],
        };

        res
    }
}

impl ShellDef {
    pub fn args_with<'a>(&'a self, additional_arg: &'a str) -> Vec<&'a str> {
        self.args
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(additional_arg))
            .collect()
    }
}

impl StringExpr {
    pub fn resolve(&self, snippet_table: &SnippetTable) -> Result<String> {
        self.inner_resolve(snippet_table, vec![])
    }

    fn inner_resolve(&self, snippet_table: &SnippetTable, parents: Vec<String>) -> Result<String> {
        let elems: Vec<_> = self
            .0
            .iter()
            .map(|x| match x {
                StringExprElem::Symbol(s) => {
                    let snip = snippet_table
                        .get(s)
                        .ok_or(anyhow!("Undefined snippet: {s}"))?;
                    let mut parents = parents.clone();
                    ensure!(
                        !parents.contains(s),
                        "Detected cycle while resolving String Expression: {parents:?}"
                    );
                    parents.push(s.clone());
                    snip.inner_resolve(snippet_table, parents)
                }
                StringExprElem::String(s) => Ok(s.clone()),
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(elems.join(""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONF: &str = r#"
        menu root {
            c: custom_commands
            f: !xa"echo "!"xa!
        }

        menu custom_commands {
            h: "print hi" - !"echo hi"!
            c: @"echo ciao"
        }
    "#;

    const ANON_CMD: &str = r#"
        menu root {
            c: cmd {
                "echo foo"
            }
        }
    "#;

    const ANON_CMD2: &str = r#"
        menu root {
            c: cmd {
                vars foo,
                    bar
                "echo $foo $bar"
            }
        }
    "#;

    const NAMED_MENU: &str = r#"
        menu root {
            m: menu2
        }

        menu "2nd menu" menu2 {
            f: "echo foo"
        }
    "#;

    const MISSING_IDENT: &str = r#"
        menu root {
            s: missing
        }
    "#;

    const NO_ROOT: &str = r#"
        menu no_root {
            a: "echo a"
        }
    "#;

    const WITH_SETTING: &str = r#"
        menu root {
            a: cmd {
                set repeat
                "touch foo"
            }
        }
    "#;

    const WITH_SETTING_2: &str = r#"
        menu root {
            a: cmd {
                set repeat, ignore_result
                "touch foo"
            }
        }
    "#;

    #[test]
    fn test_parsing() -> Result<()> {
        let root = parse(CONF)?;
        k9::snapshot!(
            root,
            r#"
Config {
    menu: Menu {
        name: "root",
        display_name: None,
        entries: {
            [
                'c',
            ]: Menu(
                Menu {
                    name: "custom_commands",
                    display_name: None,
                    entries: {
                        [
                            'c',
                        ]: Command(
                            Command {
                                exec_str: StringExpr(
                                    [
                                        String(
                                            "echo ciao",
                                        ),
                                    ],
                                ),
                                settings: [],
                                name: None,
                                shell: None,
                                env_vars: [],
                                toggle_echo_setting: true,
                            },
                        ),
                        [
                            'h',
                        ]: Command(
                            Command {
                                exec_str: StringExpr(
                                    [
                                        String(
                                            "echo hi",
                                        ),
                                    ],
                                ),
                                settings: [],
                                name: Some(
                                    "print hi",
                                ),
                                shell: None,
                                env_vars: [],
                                toggle_echo_setting: false,
                            },
                        ),
                    },
                },
            ),
            [
                'f',
            ]: Command(
                Command {
                    exec_str: StringExpr(
                        [
                            String(
                                "echo "!",
                            ),
                        ],
                    ),
                    settings: [],
                    name: None,
                    shell: None,
                    env_vars: [],
                    toggle_echo_setting: false,
                },
            ),
        },
    },
    settings: Settings {
        shell_def: None,
        echo_by_default: true,
    },
    snippet_table: {},
}
"#
        );
        Ok(())
    }

    #[test]
    fn test_missing_ident() -> Result<()> {
        let root = parse(MISSING_IDENT);
        k9::snapshot!(
            root,
            r#"
Err(
    Error {
        context: "Parsing submenu: missing",
        source: "Undefined symbol: missing",
    },
)
"#
        );
        Ok(())
    }

    #[test]
    fn test_no_root() -> Result<()> {
        let root = parse(NO_ROOT);
        k9::snapshot!(
            root,
            r#"
Err(
    "Undefined symbol: root",
)
"#
        );
        Ok(())
    }

    #[test]
    fn anon_cmd() -> Result<()> {
        let root = parse(ANON_CMD);
        k9::snapshot!(
            root,
            r#"
Ok(
    Config {
        menu: Menu {
            name: "root",
            display_name: None,
            entries: {
                [
                    'c',
                ]: Command(
                    Command {
                        exec_str: StringExpr(
                            [
                                String(
                                    "echo foo",
                                ),
                            ],
                        ),
                        settings: [],
                        name: None,
                        shell: None,
                        env_vars: [],
                        toggle_echo_setting: false,
                    },
                ),
            },
        },
        settings: Settings {
            shell_def: None,
            echo_by_default: true,
        },
        snippet_table: {},
    },
)
"#
        );
        Ok(())
    }

    #[test]
    fn anon_cmd_2_args() -> Result<()> {
        let root = parse(ANON_CMD2)?;
        k9::snapshot!(
            root,
            r#"
Config {
    menu: Menu {
        name: "root",
        display_name: None,
        entries: {
            [
                'c',
            ]: Command(
                Command {
                    exec_str: StringExpr(
                        [
                            String(
                                "echo $foo $bar",
                            ),
                        ],
                    ),
                    settings: [],
                    name: None,
                    shell: None,
                    env_vars: [
                        VarDef {
                            name: "foo",
                            value: None,
                        },
                        VarDef {
                            name: "bar",
                            value: None,
                        },
                    ],
                    toggle_echo_setting: false,
                },
            ),
        },
    },
    settings: Settings {
        shell_def: None,
        echo_by_default: true,
    },
    snippet_table: {},
}
"#
        );
        Ok(())
    }

    #[test]
    fn named_menu() -> Result<()> {
        let root = parse(NAMED_MENU)?;
        k9::snapshot!(
            root,
            r#"
Config {
    menu: Menu {
        name: "root",
        display_name: None,
        entries: {
            [
                'm',
            ]: Menu(
                Menu {
                    name: "menu2",
                    display_name: Some(
                        "2nd menu",
                    ),
                    entries: {
                        [
                            'f',
                        ]: Command(
                            Command {
                                exec_str: StringExpr(
                                    [
                                        String(
                                            "echo foo",
                                        ),
                                    ],
                                ),
                                settings: [],
                                name: None,
                                shell: None,
                                env_vars: [],
                                toggle_echo_setting: false,
                            },
                        ),
                    },
                },
            ),
        },
    },
    settings: Settings {
        shell_def: None,
        echo_by_default: true,
    },
    snippet_table: {},
}
"#
        );
        Ok(())
    }

    #[test]
    fn with_setting() -> Result<()> {
        let root = parse(WITH_SETTING)?;
        k9::snapshot!(
            root,
            r#"
Config {
    menu: Menu {
        name: "root",
        display_name: None,
        entries: {
            [
                'a',
            ]: Command(
                Command {
                    exec_str: StringExpr(
                        [
                            String(
                                "touch foo",
                            ),
                        ],
                    ),
                    settings: [
                        Repeat,
                    ],
                    name: None,
                    shell: None,
                    env_vars: [],
                    toggle_echo_setting: false,
                },
            ),
        },
    },
    settings: Settings {
        shell_def: None,
        echo_by_default: true,
    },
    snippet_table: {},
}
"#
        );
        Ok(())
    }

    #[test]
    fn with_setting2() -> Result<()> {
        let root = parse(WITH_SETTING_2)?;
        k9::snapshot!(
            root,
            r#"
Config {
    menu: Menu {
        name: "root",
        display_name: None,
        entries: {
            [
                'a',
            ]: Command(
                Command {
                    exec_str: StringExpr(
                        [
                            String(
                                "touch foo",
                            ),
                        ],
                    ),
                    settings: [
                        Repeat,
                        IgnoreResult,
                    ],
                    name: None,
                    shell: None,
                    env_vars: [],
                    toggle_echo_setting: false,
                },
            ),
        },
    },
    settings: Settings {
        shell_def: None,
        echo_by_default: true,
    },
    snippet_table: {},
}
"#
        );
        Ok(())
    }

    #[test]
    fn test_shell_parsing() {
        k9::snapshot!(
            parse_shell_string("shell bash -euo pipefail -c"),
            r#"
Ok(
    ShellDef {
        name: "bash",
        args: [
            "-euo",
            "pipefail",
            "-c",
        ],
    },
)
"#
        );
    }

    #[test]
    fn test_snippet_parsing() -> Result<()> {
        k9::snapshot!(
            parse_string_expr(
                ConfigParser::parse(Rule::string_expr, r#"$a + "b" + $c + "d""#,)?
                    .next()
                    .unwrap()
            ),
            r#"
StringExpr(
    [
        Symbol(
            "a",
        ),
        String(
            "b",
        ),
        Symbol(
            "c",
        ),
        String(
            "d",
        ),
    ],
)
"#
        );
        Ok(())
    }

    #[test]
    fn test_echo_rule() -> Result<()> {
        k9::snapshot!(
            parse_echo_setting(
                ConfigParser::parse(Rule::echo_setting, r#"echo on"#)?
                    .next()
                    .unwrap()
            ),
            "true"
        );
        k9::snapshot!(
            parse_echo_setting(
                ConfigParser::parse(Rule::echo_setting, r#"echo off"#)?
                    .next()
                    .unwrap()
            ),
            "false"
        );
        k9::snapshot!(
            ConfigParser::parse(Rule::echo_setting, r#"echo foo"#).is_err(),
            "true"
        );
        Ok(())
    }
}
