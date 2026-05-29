/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::io::{BufRead, IsTerminal, Write};
use std::process::Command;

use crate::error::Error;

pub fn resolve(
    flag_value: Option<&str>,
    env_var: &str,
    prompt_label: &str,
) -> Result<String, Error> {
    if let Some(value) = flag_value {
        return Ok(value.to_owned());
    }
    if let Ok(value) = std::env::var(env_var)
        && !value.is_empty()
    {
        return Ok(value);
    }
    if std::io::stdin().is_terminal() {
        return prompt_no_echo(prompt_label);
    }
    Err(Error::Usage(format!(
        "no {prompt_label} provided: pass the flag, set ${env_var}, or run on a TTY for an interactive prompt"
    )))
}

fn prompt_no_echo(label: &str) -> Result<String, Error> {
    let mut stderr = std::io::stderr();
    write!(stderr, "{label}: ")?;
    stderr.flush()?;

    let echo_off = set_echo(false);
    let mut line = String::new();
    let read = std::io::stdin().lock().read_line(&mut line);
    if echo_off {
        let _ = set_echo(true);
    }
    writeln!(stderr)?;
    stderr.flush()?;
    read?;

    let trimmed = line.trim_end_matches(['\r', '\n']);
    if trimmed.is_empty() {
        return Err(Error::Usage(format!("empty {label}")));
    }
    Ok(trimmed.to_owned())
}

fn set_echo(on: bool) -> bool {
    let arg = if on { "echo" } else { "-echo" };
    Command::new("stty")
        .arg(arg)
        .stdin(std::process::Stdio::inherit())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
