use anyhow::{bail, Context, Result};
use std::io::BufRead;
use std::path::Path;
use std::time::Duration;

/// ScriptCommandType identifies the type of action script command.
#[derive(Debug, Clone)]
pub enum ScriptCommand {
    Wait(Duration),
    KeyPress(String),
    KeyRelease(String),
    Screenshot(String),
}

/// Parse an action script file and return the command list.
pub fn parse_script(path: &Path) -> Result<Vec<ScriptCommand>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("opening script {}", path.display()))?;
    let reader = std::io::BufReader::new(file);
    parse_script_reader(reader)
}

/// Parse script commands from a reader.
pub fn parse_script_reader(reader: impl BufRead) -> Result<Vec<ScriptCommand>> {
    let mut commands = Vec::new();

    for (line_num, line) in reader.lines().enumerate() {
        let line = line?;
        let line = line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0] {
            "wait" => {
                if parts.len() != 2 {
                    bail!("line {}: wait requires a duration argument", line_num + 1);
                }
                let d = parse_duration(parts[1])
                    .with_context(|| format!("line {}: invalid duration {:?}", line_num + 1, parts[1]))?;
                commands.push(ScriptCommand::Wait(d));
            }
            "key" => {
                if parts.len() != 3 {
                    bail!(
                        "line {}: key requires <name> press|release",
                        line_num + 1
                    );
                }
                let key_name = parts[1].to_uppercase();
                match parts[2].to_lowercase().as_str() {
                    "press" => commands.push(ScriptCommand::KeyPress(key_name)),
                    "release" => commands.push(ScriptCommand::KeyRelease(key_name)),
                    other => bail!(
                        "line {}: key action must be press or release, got {:?}",
                        line_num + 1,
                        other
                    ),
                }
            }
            "screenshot" => {
                if parts.len() != 2 {
                    bail!(
                        "line {}: screenshot requires a file path",
                        line_num + 1
                    );
                }
                commands.push(ScriptCommand::Screenshot(parts[1].to_string()));
            }
            other => bail!(
                "line {}: unknown command {:?}",
                line_num + 1,
                other
            ),
        }
    }

    Ok(commands)
}

pub fn parse_duration(s: &str) -> Result<Duration> {
    // Go-style duration: "5s", "100ms", "1m"
    if let Some(rest) = s.strip_suffix("ms") {
        let ms: u64 = rest.parse()?;
        return Ok(Duration::from_millis(ms));
    }
    if let Some(rest) = s.strip_suffix('s') {
        let secs: f64 = rest.parse()?;
        return Ok(Duration::from_secs_f64(secs));
    }
    if let Some(rest) = s.strip_suffix('m') {
        let mins: f64 = rest.parse()?;
        return Ok(Duration::from_secs_f64(mins * 60.0));
    }
    bail!("invalid duration {:?}", s);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_parse_script() {
        let input = "# Comment\nwait 2s\nkey ENTER press\nwait 500ms\nkey ENTER release\nscreenshot out.png\n";
        let reader = std::io::BufReader::new(Cursor::new(input));
        let commands = parse_script_reader(reader).unwrap();
        assert_eq!(commands.len(), 5);

        match &commands[0] {
            ScriptCommand::Wait(d) => assert_eq!(*d, Duration::from_secs(2)),
            _ => panic!("expected Wait"),
        }
        match &commands[1] {
            ScriptCommand::KeyPress(k) => assert_eq!(k, "ENTER"),
            _ => panic!("expected KeyPress"),
        }
        match &commands[4] {
            ScriptCommand::Screenshot(p) => assert_eq!(p, "out.png"),
            _ => panic!("expected Screenshot"),
        }
    }

    #[test]
    fn test_parse_empty_script() {
        let reader = std::io::BufReader::new(Cursor::new("# only comments\n\n"));
        let commands = parse_script_reader(reader).unwrap();
        assert!(commands.is_empty());
    }

    #[test]
    fn test_parse_invalid_command() {
        let reader = std::io::BufReader::new(Cursor::new("invalid_cmd arg\n"));
        let result = parse_script_reader(reader);
        assert!(result.is_err());
    }
}
