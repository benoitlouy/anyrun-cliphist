use abi_stable::std_types::{ROption, RString, RVec};
use anyrun_plugin::*;
use fuzzy_matcher::FuzzyMatcher;
use serde::Deserialize;
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Deserialize)]
struct Config {
    #[serde(default = "max_entries")]
    max_entries: usize,
    #[serde(default = "cliphist_path")]
    cliphist_path: String,
    #[serde(default = "prefix")]
    prefix: String,
}

fn max_entries() -> usize {
    10
}

fn cliphist_path() -> String {
    "cliphist".into()
}

fn prefix() -> String {
    "".into()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_entries: max_entries(),
            cliphist_path: cliphist_path(),
            prefix: prefix(),
        }
    }
}

#[derive(Debug)]
enum Error {
    CliphistCommandFailed(std::io::Error),
    CliphistReturnCodeError(i32),
    StdinError,
    Threaderror,
}

struct State {
    config: Config,
    history: Vec<(usize, String, String)>,
}

#[init]
fn init(config_dir: RString) -> State {
    let config: Config = load_config(config_dir);

    let output = Command::new(&config.cliphist_path)
        .args(["list"])
        .output()
        .map_err(Error::CliphistCommandFailed);

    let content = match output {
        Ok(o) => {
            if o.status.success() {
                Ok(String::from_utf8_lossy(&o.stdout).into_owned())
            } else {
                Err(Error::CliphistReturnCodeError(o.status.code().unwrap_or(1)))
            }
        }
        Err(e) => Err(e),
    };

    let history = content.map(|s| {
        s.split('\n')
            .filter_map(|l| l.split_once('\t'))
            .enumerate()
            .map(|(id, (a, b))| (id, a.to_string(), b.to_string()))
            .collect::<Vec<_>>()
    });

    history.map(|history| State { config, history }).unwrap()
}

fn load_config(config_dir: RString) -> Config {
    match fs::read_to_string(format!("{}/cliphist.ron", config_dir)) {
        Ok(content) => ron::from_str(&content).unwrap_or_else(|why| {
            eprintln!("Error parsing cliphist plugin config: {}", why);
            Config::default()
        }),
        Err(why) => {
            eprintln!("Error reading cliphist plugin config: {}", why);
            Config::default()
        }
    }
}

#[info]
fn info() -> PluginInfo {
    PluginInfo {
        name: "cliphist".into(),
        icon: "view-list-symbolic".into(), // Icon from the icon theme
    }
}

#[get_matches]
fn get_matches(input: RString, state: &State) -> RVec<Match> {
    if !input.starts_with(&state.config.prefix) {
        return RVec::new();
    }

    let cleaned_input = &input[state.config.prefix.len()..];
    if cleaned_input.is_empty() {
        let max_entries = state.history.len().min(state.config.max_entries);
        let entries = &state.history[..max_entries];
        entries
            .into_iter()
            .map(|(id, _, entry)| {
                let title = entry.clone();
                Match {
                    title: title.into(),
                    description: ROption::RNone,
                    use_pango: false,
                    icon: ROption::RNone,
                    id: ROption::RSome(*id as u64),
                }
            })
            .collect()
    } else {
        let matcher = fuzzy_matcher::skim::SkimMatcherV2::default().smart_case();
        let mut entries = state
            .history
            .iter()
            .filter_map(|(id, _, entry)| {
                let score = matcher.fuzzy_match(&entry, cleaned_input).unwrap_or(0);
                if score > 0 {
                    Some((id, entry, score))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| b.2.cmp(&a.2));
        entries.truncate(state.config.max_entries);
        entries
            .into_iter()
            .map(|(id, entry, _)| {
                let title = entry.clone();
                Match {
                    title: title.into(),
                    description: ROption::RNone,
                    use_pango: false,
                    icon: ROption::RNone,
                    id: ROption::RSome(*id as u64),
                }
            })
            .collect()
    }
}

#[handler]
fn handler(selection: Match, state: &State) -> HandleResult {
    let id = state
        .history
        .iter()
        .find_map(|(id, cliphist_id, _)| {
            if *id as u64 == selection.id.unwrap() {
                Some(cliphist_id)
            } else {
                None
            }
        })
        .map(|id| format!("{}\t ", id))
        .unwrap();

    let child = Command::new(&state.config.cliphist_path)
        .args(["decode"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(Error::CliphistCommandFailed);

    let output = child.and_then(|mut c| {
        let write_to_stdin = c
            .stdin
            .take()
            .ok_or(Error::StdinError)
            .and_then(|mut stdin| {
                std::thread::spawn(move || {
                    stdin
                        .write_all(id.as_bytes())
                        .map_err(|_| Error::StdinError)
                })
                .join()
                .map_err(|_| Error::Threaderror)
                .and_then(|r| r)
            });
        write_to_stdin.and_then(|_| c.wait_with_output().map_err(Error::CliphistCommandFailed))
    });

    output
        .map(|bytes| HandleResult::Copy(bytes.stdout.into()))
        .unwrap()
}
