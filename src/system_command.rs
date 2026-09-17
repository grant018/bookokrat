use std::process::Command;

/// Trait for executing system commands (mockable for testing)
pub trait SystemCommandExecutor {
    fn open_file(&self, path: &str) -> Result<(), String>;
    fn open_file_at_chapter(&self, path: &str, chapter: usize) -> Result<(), String>;
    fn open_url(&self, url: &str) -> Result<(), String>;
    fn as_any(&self) -> &dyn std::any::Any;
}

pub struct RealSystemCommandExecutor;

impl SystemCommandExecutor for RealSystemCommandExecutor {
    fn open_file(&self, path: &str) -> Result<(), String> {
        self.open_file_at_chapter(path, 0)
    }

    fn open_file_at_chapter(&self, path: &str, chapter: usize) -> Result<(), String> {
        use std::path::PathBuf;

        let absolute_path = if std::path::Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            std::env::current_dir()
                .map_err(|e| format!("Failed to get current directory: {e}"))?
                .join(path)
        };

        if !absolute_path.exists() {
            return Err(format!("File does not exist: {}", absolute_path.display()));
        }

        let absolute_path_str = absolute_path.to_string_lossy();

        let result = if cfg!(target_os = "macos") {
            self.open_with_macos_epub_reader(absolute_path_str.as_ref(), chapter)
                .or_else(|_| Command::new("open").arg(absolute_path_str.as_ref()).spawn())
        } else if cfg!(target_os = "windows") {
            self.open_with_windows_epub_reader(absolute_path_str.as_ref(), chapter)
                .or_else(|_| {
                    Command::new("cmd")
                        .args(["/C", "start", "", absolute_path_str.as_ref()])
                        .spawn()
                })
        } else {
            self.open_with_linux_epub_reader(absolute_path_str.as_ref(), chapter)
                .or_else(|_| {
                    Command::new("xdg-open")
                        .arg(absolute_path_str.as_ref())
                        .spawn()
                })
        };

        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(format!(
                "Failed to open file '{}': {}",
                absolute_path.display(),
                e
            )),
        }
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        let (command, args) = if cfg!(target_os = "macos") {
            ("open", vec![url])
        } else if cfg!(target_os = "windows") {
            ("cmd", vec!["/C", "start", "", url])
        } else {
            // Linux and other Unix-like systems
            ("xdg-open", vec![url])
        };

        Command::new(command)
            .args(&args)
            .spawn()
            .map_err(|e| format!("Failed to open URL: {e}"))?;

        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl RealSystemCommandExecutor {
    fn open_with_macos_epub_reader(
        &self,
        path: &str,
        chapter: usize,
    ) -> Result<std::process::Child, std::io::Error> {
        if let Ok(child) = self.try_calibre_viewer(path, chapter) {
            return Ok(child);
        }
        if let Ok(child) = self.try_clearview(path, chapter) {
            return Ok(child);
        }
        if let Ok(child) = self.try_skim(path, chapter) {
            return Ok(child);
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No compatible EPUB reader found",
        ))
    }

    fn open_with_windows_epub_reader(
        &self,
        path: &str,
        chapter: usize,
    ) -> Result<std::process::Child, std::io::Error> {
        if let Ok(child) = self.try_calibre_viewer(path, chapter) {
            return Ok(child);
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No compatible EPUB reader found",
        ))
    }

    fn open_with_linux_epub_reader(
        &self,
        path: &str,
        chapter: usize,
    ) -> Result<std::process::Child, std::io::Error> {
        if let Ok(child) = self.try_calibre_viewer(path, chapter) {
            return Ok(child);
        }
        if let Ok(child) = self.try_fbreader(path, chapter) {
            return Ok(child);
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No compatible EPUB reader found",
        ))
    }

    fn try_clearview(
        &self,
        path: &str,
        _chapter: usize,
    ) -> Result<std::process::Child, std::io::Error> {
        Command::new("open").args(["-a", "ClearView", path]).spawn()
    }

    fn try_calibre_viewer(
        &self,
        path: &str,
        chapter: usize,
    ) -> Result<std::process::Child, std::io::Error> {
        if chapter > 0 {
            let chapter_patterns = [
                format!("toc:Chapter {}", chapter + 1),
                format!("toc:Ch {}", chapter + 1),
                format!("toc:{}", chapter + 1),
                format!("toc:Chapter{}", chapter + 1),
            ];

            for pattern in &chapter_patterns {
                if let Ok(child) = Command::new("ebook-viewer")
                    .arg(format!("--goto={pattern}"))
                    .arg(path)
                    .spawn()
                {
                    return Ok(child);
                }
            }
        }
        Command::new("ebook-viewer").arg(path).spawn()
    }

    fn try_skim(&self, path: &str, _chapter: usize) -> Result<std::process::Child, std::io::Error> {
        Command::new("open").args(["-a", "Skim", path]).spawn()
    }

    fn try_fbreader(
        &self,
        path: &str,
        _chapter: usize,
    ) -> Result<std::process::Child, std::io::Error> {
        Command::new("fbreader").arg(path).spawn()
    }
}

/// Build a command that runs `command_line` through the platform shell.
///
/// On Windows, `/S` makes cmd strip exactly the outer quote pair and take the
/// rest verbatim, and `raw_arg` bypasses the MSVCRT-style quoting std would
/// otherwise apply — cmd.exe does not follow those rules.
#[cfg(windows)]
pub fn shell_command(command_line: &str) -> Command {
    use std::os::windows::process::CommandExt;

    // A spawned console process inherits our console, and redirecting the std
    // handles does not stop it touching that console directly: cmd and
    // powershell scroll the buffer on startup, which desynchronizes the
    // diffing renderer and leaves shifted text behind until a full repaint.
    // CREATE_NO_WINDOW gives the child its own hidden console instead.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut command = Command::new("cmd");
    command
        .arg("/S")
        .arg("/C")
        .raw_arg(format!("\"{command_line}\""))
        .creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(not(windows))]
pub fn shell_command(command_line: &str) -> Command {
    let mut command = Command::new("sh");
    command.arg("-c").arg(command_line);
    command
}

/// Escape `text` for substitution into a quoted placeholder in a user-supplied
/// command template. cmd.exe escapes an embedded double quote by doubling it;
/// sh has to close, escape, and reopen the single quote.
#[cfg(windows)]
pub fn shell_escape(text: &str) -> String {
    text.replace('"', "\"\"")
}

#[cfg(not(windows))]
pub fn shell_escape(text: &str) -> String {
    text.replace('\'', "'\\''")
}

/// Replace ASCII double quotes with typographic ones.
///
/// A Windows command line is parsed twice — by cmd, then by the target's own
/// argument parser — and an ASCII double quote does not survive both: the
/// MSVCRT doubling that `shell_escape` applies is correct for an ordinary
/// executable but ignored by `powershell.exe -File`, which cuts the value
/// short at the quote. Curly quotes are inert to both parsers and read the
/// same in a lookup.
#[cfg(windows)]
pub fn soften_quotes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut opening = true;
    for ch in text.chars() {
        if ch == '"' {
            out.push(if opening { '\u{201C}' } else { '\u{201D}' });
            opening = !opening;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Quote `text` as a standalone argument appended to a command line.
#[cfg(windows)]
pub fn shell_quote(text: &str) -> String {
    format!("\"{}\"", shell_escape(text))
}

#[cfg(not(windows))]
pub fn shell_quote(text: &str) -> String {
    format!("'{}'", shell_escape(text))
}

#[cfg(any(test, feature = "test-utils"))]
pub struct MockSystemCommandExecutor {
    pub executed_commands: std::cell::RefCell<Vec<String>>,
    pub should_fail: bool,
}

#[cfg(any(test, feature = "test-utils"))]
impl Default for MockSystemCommandExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl MockSystemCommandExecutor {
    pub fn new() -> Self {
        Self {
            executed_commands: std::cell::RefCell::new(Vec::new()),
            should_fail: false,
        }
    }

    pub fn get_executed_commands(&self) -> Vec<String> {
        self.executed_commands.borrow().clone()
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl SystemCommandExecutor for MockSystemCommandExecutor {
    fn open_file(&self, path: &str) -> Result<(), String> {
        self.executed_commands.borrow_mut().push(path.to_string());
        if self.should_fail {
            Err("Mock failure".to_string())
        } else {
            Ok(())
        }
    }

    fn open_file_at_chapter(&self, path: &str, chapter: usize) -> Result<(), String> {
        self.executed_commands
            .borrow_mut()
            .push(format!("{path}@chapter{chapter}"));
        if self.should_fail {
            Err("Mock failure".to_string())
        } else {
            Ok(())
        }
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        self.executed_commands
            .borrow_mut()
            .push(format!("URL:{url}"));
        if self.should_fail {
            Err("Mock failure".to_string())
        } else {
            Ok(())
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
