use std::collections::HashMap;

use pest::{
    iterators::{Pair, Pairs},
    Parser,
};
use pest_derive::Parser;

use anyhow::{anyhow, Context, Result};

use crate::core::{Command, Menu, Node};

#[derive(Parser)]
#[grammar = "grammar.pest"]
struct ConfigParser;

pub fn parse(src: &str) -> Result<Menu> {
    let mut pairs = ConfigParser::parse(Rule::file, src).context("Parsing source")?;
    let file = pairs.next().unwrap();
    assert!(file.as_rule() == Rule::file);
    let menus = file.into_inner();
    // println!("{menus:#?}");
    let symbols = get_symbol_table(menus);
    parse_menu("root", &symbols)
}

fn get_symbol_table<'a>(pairs: Pairs<'a, Rule>) -> HashMap<&'a str, Pairs<'a, Rule>> {
    pairs
        .into_iter()
        .filter(|x| x.as_rule() != Rule::EOI)
        .map(|menu| {
            let mut menu_elems = menu.into_inner();
            let menu_name = menu_elems.next().unwrap();
            (menu_name.as_str(), menu_elems.next().unwrap().into_inner())
        })
        .collect()
}

fn parse_menu(name: &str, menus: &HashMap<&str, Pairs<'_, Rule>>) -> Result<Menu> {
    let mut entries = HashMap::new();
    for entry in menus
        .get(name)
        .ok_or(anyhow!("Undefined symbol: {name}"))?
        .clone()
    {
        let mut children = entry.into_inner();
        let keys = children.next().unwrap().as_str().to_string();
        let child_pair = children.next().unwrap();
        let next_node = match child_pair.as_rule() {
            Rule::symbol => {
                let submenu_name = child_pair.as_str();
                Node::Menu(
                    parse_menu(submenu_name, menus)
                        .context(format!("Parsing submenu: {submenu_name}"))?,
                )
            }
            Rule::quick_command => parse_quick_command(child_pair)?,
            _ => {
                panic!("unexpected rule: {child_pair:?}")
            }
        };
        entries.insert(keys, next_node);
    }
    Ok(Menu {
        name: name.to_string(),
        entries,
    })
}
fn parse_quick_command(pair: Pair<'_, Rule>) -> Result<Node> {
    let elems: Vec<_> = pair.into_inner().map(get_string_content).collect();
    let cmd = match elems.len() {
        1 => Command {
            exec_str: elems[0].clone(),
            name: None,
        },
        2 => Command {
            exec_str: elems[1].clone(),
            name: Some(elems[0].clone()),
        },
        _ => panic!("unexpected amount of string"),
    };
    Ok(Node::Command(cmd))
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

    // TODO: implement check so this will fail a test
    const _PREFIX_KEYS: &str = r#"
        menu root {
            a: !"echo a"!
            aa: !"echo aa"!
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
    entries: {
        "c": Menu(
            Menu {
                name: "custom_commands",
                entries: {
                    "c": Command(
                        Command {
                            exec_str: "echo ciao",
                            name: None,
                        },
                    ),
                    "h": Command(
                        Command {
                            exec_str: "echo hi",
                            name: Some(
                                "print hi",
                            ),
                        },
                    ),
                },
            },
        ),
        "f": Command(
            Command {
                exec_str: "echo "!",
                name: None,
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
}
