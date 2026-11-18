//! Lookups on selected text. Targets come from the terminal app's
//! `config.yaml` and run through the same shell quoting, so a target behaves
//! the same from either app.

use bookokrat::settings::{LookupDisplay, LookupTarget, preferred_config_dir};
use bookokrat::system_command::{fill_placeholder, reap_in_background, shell_command, shell_quote};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::process::Stdio;
use std::sync::Mutex;

#[derive(Default, Deserialize)]
#[serde(default)]
struct LookupConfig {
    lookup_command: Option<String>,
    lookup_display: LookupDisplay,
    lookups: BTreeMap<String, LookupTarget>,
}

#[derive(Serialize)]
pub struct TargetDto {
    name: String,
    display: LookupDisplay,
}

#[derive(Serialize)]
pub struct Targets {
    targets: Vec<TargetDto>,
    /// How the single `lookup_command` shows its result, when one is set. It
    /// is what runs when no named targets are configured.
    fallback: Option<LookupDisplay>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    Launched,
    Output { ok: bool, text: String },
    Cancelled,
}

/// Lookups whose output is still awaited, by the id the frontend gave them,
/// with the process group to signal if the reader stops waiting.
#[derive(Default)]
pub struct Running(Mutex<HashMap<u64, i32>>);

impl Running {
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<u64, i32>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn cancel(&self, id: u64) {
        if let Some(group) = self.map().remove(&id) {
            terminate(group);
        }
    }

    pub fn cancel_all(&self) {
        for (_, group) in self.map().drain() {
            terminate(group);
        }
    }
}

#[cfg(unix)]
fn terminate(group: i32) {
    // SAFETY: plain syscall; a group that has already exited is reported as
    // an error and ignored.
    unsafe {
        libc::kill(-group, libc::SIGTERM);
    }
}

#[cfg(not(unix))]
fn terminate(_group: i32) {}

fn load() -> Result<LookupConfig, String> {
    let Some(path) = preferred_config_dir().map(|dir| dir.join("config.yaml")) else {
        return Ok(LookupConfig::default());
    };
    if !path.exists() {
        return Ok(LookupConfig::default());
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("Could not read config.yaml: {e}"))
        .inspect_err(|e| log::error!("{e}"))?;
    serde_yaml::from_str(&text)
        .map_err(|e| format!("Could not read lookups from config.yaml: {e}"))
        .inspect_err(|e| log::error!("{e}"))
}

pub fn targets() -> Result<Targets, String> {
    let config = load()?;
    Ok(Targets {
        targets: config
            .lookups
            .into_iter()
            .map(|(name, target)| TargetDto {
                name,
                display: target.display,
            })
            .collect(),
        fallback: config.lookup_command.map(|_| config.lookup_display),
    })
}

/// Runs `target` (or the single `lookup_command` when `None`) on `text`.
/// For a target shown in the reader this blocks until the command finishes,
/// so call it off the main thread.
pub fn run(
    running: &Running,
    id: u64,
    target: Option<&str>,
    text: &str,
) -> Result<Outcome, String> {
    let config = load()?;
    let (template, display) = match target {
        Some(name) => {
            let target = config
                .lookups
                .get(name)
                .ok_or_else(|| format!("Lookup target '{name}' is no longer configured"))?;
            (target.command.clone(), target.display)
        }
        None => (
            config
                .lookup_command
                .ok_or("No lookup command configured. Set lookup_command in config.yaml.")?,
            config.lookup_display,
        ),
    };

    // A selection can span blocks, and a newline would end the command early
    // on Windows, so the text goes in as one line.
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Err("No text selected".to_string());
    }
    #[cfg(windows)]
    let normalized = bookokrat::system_command::soften_quotes(&normalized);

    let command_line = if template.contains("{}") {
        fill_placeholder(&template, "{}", &normalized)
    } else {
        format!("{template} {}", shell_quote(&normalized))
    };
    let mut command = shell_command(&command_line);
    command.stdin(Stdio::null());

    match display {
        LookupDisplay::FireAndForget => {
            let child = command
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("Failed to launch command: {e}"))?;
            reap_in_background(child);
            Ok(Outcome::Launched)
        }
        LookupDisplay::Popup => {
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
            // Its own process group, so cancelling stops the whole pipeline
            // rather than just the shell that launched it.
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                command.process_group(0);
            }
            let child = command
                .spawn()
                .map_err(|e| format!("Failed to run command: {e}"))?;
            running.map().insert(id, child.id() as i32);
            let output = child.wait_with_output();
            // Gone from the map means the reader cancelled it meanwhile.
            if running.map().remove(&id).is_none() {
                return Ok(Outcome::Cancelled);
            }

            let output = output.map_err(|e| format!("Failed to run command: {e}"))?;
            if output.status.success() {
                Ok(Outcome::Output {
                    ok: true,
                    text: String::from_utf8_lossy(&output.stdout).into_owned(),
                })
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
                Ok(Outcome::Output {
                    ok: false,
                    text: if stderr.trim().is_empty() {
                        format!("Command exited with status {}", output.status)
                    } else {
                        stderr
                    },
                })
            }
        }
    }
}
