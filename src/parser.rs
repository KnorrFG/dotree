use std::collections::HashMap;

use pest::{
    iterators::{Pair, Pairs},
    Parser,
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
    pub name: Option<String>,
    pub env_vars: Vec<String>,
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
            Rule::quick_command => Node::Command(parse_quick_command(child_pair)?),
            Rule::anon_command => Node::Command(parse_anon_command(child_pair)?),
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

fn parse_anon_command(p: Pair<'_, Rule>) -> Result<Command> {
    let body = p.into_inner().next().unwrap();
    let mut elems = body.into_inner();
    let first = elems.next().unwrap();
    match first.as_rule() {
        Rule::vars_def => {
            let env_vars = parse_vars_def(first);
            let mut cmd = parse_quick_command(elems.next().unwrap())?;
            cmd.env_vars = env_vars;
            Ok(cmd)
        }
        Rule::quick_command => parse_quick_command(first),
        _ => panic!("unexpected rule: {first:#?}"),
    }
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

fn parse_quick_command(pair: Pair<'_, Rule>) -> Result<Command> {
    assert!(pair.as_rule() == Rule::quick_command);
    let elems: Vec<_> = pair.into_inner().map(get_string_content).collect();
    let cmd = match elems.len() {
        1 => Command {
            exec_str: elems[0].clone(),
            name: None,
            env_vars: vec![],
        },
        2 => Command {
            exec_str: elems[1].clone(),
            name: Some(elems[0].clone()),
            env_vars: vec![],
        },
        _ => panic!("unexpected amount of string"),
    };
    Ok(cmd)
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
                            name: None,
                            env_vars: [],
                        },
                    ),
                    [
                        'h',
                    ]: Command(
                        Command {
                            exec_str: "echo hi",
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
}
