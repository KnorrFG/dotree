use std::{env, fs, path::PathBuf};

use anyhow::{ensure, Result};
use subprocess::{Exec, Redirection};

#[test]
fn bash_tests() -> Result<()> {
    ensure!(
        Exec::shell("cargo b").join()?.success(),
        "Can't compile dotree"
    );

    let bash_files = get_bash_files()?;
    let bin_path = fs::canonicalize("target/debug/dt")?;
    env::set_var("DT", bin_path.into_os_string());
    for bash_file in bash_files {
        let output = Exec::cmd("bash")
            .arg(bash_file.file_name().unwrap())
            .cwd("tests/bash_tests")
            .stderr(Redirection::Merge)
            .stdout(Redirection::Pipe)
            .capture()?
            .stdout_str();
        let out_file_name = bash_file.with_extension("out");
        if out_file_name.exists() {
            let contents = fs::read_to_string(out_file_name)?;
            ensure!(
                contents == output,
                "test {} failed. Outputs differ. New output: \n\n{}",
                bash_file.to_string_lossy(),
                output
            );
        } else {
            fs::write(out_file_name, output)?;
        }
    }
    Ok(())
}

fn get_bash_files() -> Result<Vec<PathBuf>> {
    let mut res = vec![];
    for f in fs::read_dir("tests/bash_tests")? {
        let f = f?.path();
        if f.extension().and_then(|x| x.to_str()) == Some("bash") {
            res.push(f);
        }
    }
    Ok(res)
}
