//! A console's command, as a list of arguments and the folder it runs in: the terminal
//! (`terminal.rs`) runs it as it is, no shell reading its words.

use std::path::{Path, PathBuf};

/// What a console runs: a program, its arguments, and the folder it runs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Console {
    pub dir: Option<PathBuf>,
    pub argv: Vec<String>,
}

impl Console {
    pub fn new(argv: &[&str]) -> Self {
        Console {
            dir: None,
            argv: argv.iter().map(|a| a.to_string()).collect(),
        }
    }

    pub fn in_dir(self, dir: &Path) -> Self {
        Console {
            dir: Some(dir.to_path_buf()),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_console_runs_its_words_as_they_are_in_its_folder() {
        let c = Console::new(&["vagrant", "ssh", "pc"]).in_dir(Path::new("/labs/it's"));
        assert_eq!(c.argv, ["vagrant", "ssh", "pc"]);
        assert_eq!(c.dir.as_deref(), Some(Path::new("/labs/it's")));
        assert_eq!(
            Console::new(&["docker", "exec", "-it", "abc", "sh"]).dir,
            None
        );
    }
}
