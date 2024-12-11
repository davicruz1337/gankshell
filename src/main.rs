use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use rustyline::{Editor, error::ReadlineError, Config as RlCfg};
use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::Helper;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use serde::{Serialize, Deserialize};
use chrono::Local;
use whoami;

#[derive(Debug, Serialize, Deserialize)]
struct Cfg {
    ps: String,
    al: HashMap<String, String>,
}

impl Default for Cfg {
    fn default() -> Self {
        Self {
            ps: "->".to_string(),
            al: HashMap::new(),
        }
    }
}

struct Comp;

impl Validator for Comp {
    fn validate(&self, _: &mut ValidationContext) -> Result<ValidationResult, ReadlineError> {
        Ok(ValidationResult::Valid(None))
    }
}

impl Completer for Comp {
    type Candidate = Pair;

    fn complete(&self, line: &str, pos: usize, _: &rustyline::Context<'_>) -> Result<(usize, Vec<Self::Candidate>), ReadlineError> {
        let cmds = vec!["ls", "cd", "exit", "alias", "env", "cat", "source"];
        let prefix = &line[..pos];
        let mut matches: Vec<Self::Candidate> = cmds
            .iter()
            .filter(|c| c.starts_with(prefix))
            .map(|c| Pair {
                display: c.to_string(),
                replacement: c.to_string(),
            })
            .collect();

        if let Ok(paths) = env::var("PATH") {
            for path in paths.split(':') {
                if let Ok(entries) = fs::read_dir(path) {
                    for entry in entries.filter_map(|e| e.ok()) {
                        if let Some(name) = entry.file_name().to_str() {
                            if name.starts_with(prefix) {
                                matches.push(Pair {
                                    display: name.to_string(),
                                    replacement: name.to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }

        if let Ok(entries) = fs::read_dir(".") {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with(prefix) {
                        matches.push(Pair {
                            display: name.to_string(),
                            replacement: name.to_string(),
                        });
                    }
                }
            }
        }

        Ok((0, matches))
    }
}

impl Hinter for Comp {
    type Hint = String;
    fn hint(&self, _: &str, _: usize, _: &rustyline::Context<'_>) -> Option<Self::Hint> {
        None
    }
}

impl Highlighter for Comp {}
impl Helper for Comp {}

struct Sh {
    cfg: Cfg,
    cfg_path: String,
    ed: Editor<Comp>,
}

impl Sh {
    fn new() -> Self {
        let home = env::var("HOME").unwrap_or_else(|_| "./".to_string());
        let cfg_path = format!("{}/.sh_cfg", home);

        let cfg = if Path::new(&cfg_path).exists() {
            fs::read_to_string(&cfg_path)
                .ok()
                .and_then(|c| toml::from_str(&c).ok())
                .unwrap_or_default()
        } else {
            Cfg::default()
        };

        let rl_cfg = RlCfg::builder().auto_add_history(true).build();
        let mut ed = Editor::with_config(rl_cfg);
        ed.set_helper(Some(Comp));

        Self { cfg, cfg_path, ed }
    }

    fn save(&self) {
        if let Ok(cfg_str) = toml::to_string(&self.cfg) {
            if let Err(e) = fs::write(&self.cfg_path, cfg_str) {
                eprintln!("Erro ao salvar: {:?}", e);
            }
        }
    }

    fn run(&mut self) {
        loop {
            let user = whoami::username();
            let cwd = env::current_dir().unwrap_or_else(|_| Path::new("/").to_path_buf());
            let time = Local::now().format("%H:%M:%S");
            let ps = format!(
                "\x1b[1;32m{}@{}\x1b[0m \x1b[1;34m{}\x1b[0m [{}] {} ",
                user,
                whoami::hostname(),
                cwd.display(),
                time,
                self.cfg.ps
            );

            match self.ed.readline(&ps) {
                Ok(line) => {
                    if line.trim().is_empty() {
                        continue;
                    }
                    self.handle(line);
                }
                Err(ReadlineError::Interrupted) => println!("\nInterrompido"),
                Err(ReadlineError::Eof) => {
                    println!("\nSaindo...");
                    break;
                }
                Err(e) => {
                    println!("Erro: {:?}", e);
                    break;
                }
            }
        }
    }

    fn expand_path(&self, path: &str) -> String {
        if path.starts_with('~') {
            if let Ok(home) = env::var("HOME") {
                return path.replacen("~", &home, 1);
            }
        }
        path.to_string()
    }

    fn handle(&mut self, input: String) {
        let mut parts = input.trim().split_whitespace();
        if let Some(cmd) = parts.next() {
            match cmd {
                "exit" => {
                    self.save();
                    println!("Tchau!");
                    std::process::exit(0);
                }
                "alias" => {
                    if let Some((k, v)) = parts.collect::<Vec<&str>>().join(" ").split_once('=') {
                        self.cfg.al.insert(k.to_string(), v.to_string());
                        println!("Alias adicionado: {} -> {}", k, v);
                    } else {
                        println!("Uso: alias nome=comando");
                    }
                }
                "env" => {
                    for (k, v) in env::vars() {
                        println!("{}={}", k, v);
                    }
                }
                "cd" => {
                    if let Some(dir) = parts.next() {
                        let expanded = self.expand_path(dir);
                        if let Err(e) = env::set_current_dir(expanded) {
                            println!("Erro: {}", e);
                        }
                    } else {
                        println!("Uso: cd <diretório>");
                    }
                }
                "source" => {
                    if let Some(file) = parts.next() {
                        let expanded = self.expand_path(file);
                        if let Ok(content) = fs::read_to_string(expanded) {
                            for line in content.lines() {
                                self.handle(line.to_string());
                            }
                        } else {
                            println!("Erro ao carregar o arquivo {}", file);
                        }
                    } else {
                        println!("Uso: source <arquivo>");
                    }
                }
                _ => {
                    let cmd = self.cfg.al.get(cmd).cloned().unwrap_or_else(|| cmd.to_string());
                    let args: Vec<&str> = parts.collect();
                    self.exec(&cmd, &args);
                }
            }
        }
    }

    fn exec(&self, cmd: &str, args: &[&str]) {
        match Command::new(cmd)
            .args(args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
        {
            Ok(status) => {
                if !status.success() {
                    println!("Falhou: {:?}", status);
                }
            }
            Err(e) => println!("Erro ao executar: {:?}", e),
        }
    }
}

fn main() {
    let mut sh = Sh::new();
    sh.run();
}
