//! Minimal stdin interaction.

use std::io::{self, BufRead, Write};

use anyhow::{bail, Result};

pub fn read_line(question: &str) -> Result<String> {
    print!("{question}");
    io::stdout().flush()?;
    let mut buf = String::new();
    let read = io::stdin().lock().read_line(&mut buf)?;
    if read == 0 {
        bail!("unexpected end of input (stdin closed)");
    }
    Ok(buf.trim().to_string())
}

/// Pick one entry from a numbered list. Returns the index.
pub fn select(question: &str, count: usize, default: Option<usize>) -> Result<usize> {
    if count == 0 {
        bail!("nothing to choose from");
    }
    if count == 1 {
        println!("{question}: only one option, picking 0");
        return Ok(0);
    }
    let hint = match default {
        Some(d) => format!(" [Enter = {d}]"),
        None => String::new(),
    };
    loop {
        let answer = read_line(&format!("{question} (0-{}){hint}: ", count - 1))?;
        if answer.is_empty() {
            if let Some(d) = default {
                return Ok(d);
            }
        }
        match answer.parse::<usize>() {
            Ok(i) if i < count => return Ok(i),
            _ => println!("  Enter a number between 0 and {}.", count - 1),
        }
    }
}

/// Wait for Enter.
pub fn wait_enter(message: &str) -> Result<()> {
    read_line(&format!("{message} (Enter) "))?;
    Ok(())
}
