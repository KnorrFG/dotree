use std::collections::HashMap;

use pest::{
    iterators::{Pair, Pairs},
    ParseResult, Parser,
};
use pest_derive::Parser;

use anyhow::{anyhow, Context, Result};

#[derive(Parser)]
#[grammar = "grammar.pest"]
struct ConfigParser;

#[derive(Debug)]
pub enum Node {
    Menu(Menu),
    Command(Command),
}

#[derive(Debug)]
pub struct Menu {
    pub name: String,
    pub display_name: Option<String>,
    pub entries: HashMap<Vec<char>, Node>,
}

#[derive(Debug)]
pub struct Command {
    pub exec_str: String,
    pub settings: Vec<CommandSetting>,
    pub name: Option<String>,
    pub env_vars: Vec<String>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum CommandSetting {
    Repeat,
    IgnoreResult,
}

#[derive(Debug, Clone)]
struct RawMenu<'a> {
    display_name: Option<String>,
    body: Pairs<'a, Rule>,
}

pub fn parse(src: &str) -> Result<Menu> {
    let mut pairs = ConfigParser::parse(Rule::file, src).context("Parsing source")?;
    let file = pairs.next().unwrap();
    assert!(file.as_rule() == Rule::file);
    let menus = file.into_inner();
    // println!("{menus:#?}");
    let symbols = get_symbol_table(menus);
    parse_menu("root", &symbols)
}

fn get_symbol_table(pairs: Pairs<'_, Rule>) -> HashMap<&str, RawMenu<'_>> {
    pairs
        .into_iter()
        .filter(|x| x.as_rule() != Rule::EOI)
        .map(|menu| {
            let mut menu_elems = menu.into_inner();
            let first_child = menu_elems.next().unwrap();
            let (display_name, menu_name) = if first_child.as_rule() == Rule::string {
                (
                    Some(
                        first_child
                            .into_inner()
                            .next()
                            .unwrap()
                            .into_inner()
                            .next()
                            .unwrap()
                            .as_str()
                            .to_string(),
                    ),
                    menu_elems.next().unwrap(),
                )
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
                let (display_name, exec_str) = parse_quick_command(child_pair);
                Node::Command(Command {
                    exec_str,
                    name: display_name,
                    settings: vec![],
                    env_vars: vec![],
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
    let body = p.into_inner().next().unwrap();
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
    vars: Option<Vec<String>>,
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
            Rule::quick_command => {
                let (display_name, exec_str) = parse_quick_command(p);
                Some(Command {
                    exec_str,
                    settings: self.settings.take().unwrap_or(vec![]),
                    name: display_name,
                    env_vars: self.vars.take().unwrap_or(vec![]),
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

fn parse_vars_def(p: Pair<'_, Rule>) -> Vec<String> {
    assert!(p.as_rule() == Rule::vars_def);
    p.into_inner()
        .map(|p| {
            assert!(p.as_rule() == Rule::var_def, "unexpected rule: {p:#?}");
            p.into_inner().next().unwrap().as_str().to_string()
        })
        .collect()
}

fn parse_quick_command(pair: Pair<'_, Rule>) -> (Option<String>, String) {
    assert!(pair.as_rule() == Rule::quick_command);
    let elems: Vec<_> = pair.into_inner().map(get_string_content).collect();
    match elems.len() {
        1 => (None, elems[0].clone()),
        2 => (Some(elems[0].clone()), elems[1].clone()),
        _ => panic!("unexpected amount of string"),
    }
}

fn get_string_content(p: Pair<'_, Rule>) -> String {
    let normal_or_protected = p.into_inner().next().unwrap();
    let res = normal_or_protected
        .into_inner()
        .next()
        .unwrap()
        .as_str()
        .to_string();
    res
}

impl std::fmt::Display for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Menu(m) => write!(f, "{}", m.display_name.as_ref().unwrap_or(&m.name)),
            Self::Command(c) => write!(f, "{c}"),
        }
    }
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

impl Command {
    pub fn repeat(&self) -> bool {
        self.settings.contains(&CommandSetting::Repeat)
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
            c: "echo ciao"
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
Menu {
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
                            exec_str: "echo ciao",
                            settings: [],
                            name: None,
                            env_vars: [],
                        },
                    ),
                    [
                        'h',
                    ]: Command(
                        Command {
                            exec_str: "echo hi",
                            settings: [],
                            name: Some(
                                "print hi",
                            ),
                            env_vars: [],
                        },
                    ),
                },
            },
        ),
        [
            'f',
        ]: Command(
            Command {
                exec_str: "echo "!",
                settings: [],
                name: None,
                env_vars: [],
            },
        ),
    },
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
    Menu {
        name: "root",
        display_name: None,
        entries: {
            [
                'c',
            ]: Command(
                Command {
                    exec_str: "echo foo",
                    settings: [],
                    name: None,
                    env_vars: [],
                },
            ),
        },
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
Menu {
    name: "root",
    display_name: None,
    entries: {
        [
            'c',
        ]: Command(
            Command {
                exec_str: "echo $foo $bar",
                settings: [],
                name: None,
                env_vars: [
                    "foo",
                    "bar",
                ],
            },
        ),
    },
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
Menu {
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
                            exec_str: "echo foo",
                            settings: [],
                            name: None,
                            env_vars: [],
                        },
                    ),
                },
            },
        ),
    },
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
Menu {
    name: "root",
    display_name: None,
    entries: {
        [
            'a',
        ]: Command(
            Command {
                exec_str: "touch foo",
                settings: [
                    Repeat,
                ],
                name: None,
                env_vars: [],
            },
        ),
    },
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
Menu {
    name: "root",
    display_name: None,
    entries: {
        [
            'a',
        ]: Command(
            Command {
                exec_str: "touch foo",
                settings: [
                    Repeat,
                    IgnoreResult,
                ],
                name: None,
                env_vars: [],
            },
        ),
    },
}
"#
        );
        Ok(())
    }
}
