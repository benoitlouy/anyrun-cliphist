use abi_stable::std_types::{ROption, RString, RVec};
use anyrun_plugin::*;
use fuzzy_matcher::FuzzyMatcher;
use itertools::Itertools;
use nut::{DBBuilder, DB};
use serde::Deserialize;
use std::fs;

const BUCKET_NAME: &str = "b";

#[derive(Deserialize)]
struct Config {
    max_entries: usize,
    db_path: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_entries: 10,
            db_path: None,
        }
    }
}

#[derive(Debug)]
enum Error {
    CacheDirNotFound,
    DBReadError { _msg: String, _cause: nut::Error },
    DBTxError(nut::Error),
    DBBucketError(nut::Error),
    DBCursorError(nut::Error),
}

struct State {
    config: Config,
    history: Vec<(u64, String)>,
}

#[init]
fn init(config_dir: RString) -> State {
    let config: Config = load_config(config_dir);

    let db_path = match config.db_path {
        Some(ref s) => Ok(std::path::Path::new(s).to_path_buf()),
        None => dirs::cache_dir()
            .ok_or(Error::CacheDirNotFound)
            .map(|d| d.as_path().join("cliphist").join("db")),
    };

    let db = db_path.and_then(|path| {
        DBBuilder::new(path.clone())
            .read_only(true)
            .build()
            .map_err(|e| Error::DBReadError {
                _msg: format!("failed opening cliphist db at {}", path.display()),
                _cause: e,
            })
    });

    db.and_then(get_clipboard_history)
        .map(|history| State { config, history })
        .unwrap()
}

fn load_config(config_dir: RString) -> Config {
    match fs::read_to_string(format!("{}/cliphist.ron", config_dir)) {
        Ok(content) => ron::from_str(&content).unwrap_or_else(|why| {
            eprintln!("Error parsing applications plugin config: {}", why);
            Config::default()
        }),
        Err(why) => {
            eprintln!("Error reading applications plugin config: {}", why);
            Config::default()
        }
    }
}

fn get_clipboard_history(db: DB) -> Result<Vec<(u64, String)>, Error> {
    db.begin_tx().map_err(Error::DBTxError).and_then(|tx| {
        tx.bucket(BUCKET_NAME.as_bytes())
            .map_err(Error::DBBucketError)
            .and_then(|bucket| {
                bucket.cursor().map_err(Error::DBCursorError).map(|cursor| {
                    let mut res = Vec::new();
                    let mut id = 0;
                    if let Ok(item) = cursor.first() {
                        if let Some(v) = item.value {
                            if let Ok(s) = String::from_utf8(v.to_vec()) {
                                res.push((id, s));
                                id += 1;
                            }
                        }
                        while let Ok(item) = cursor.next() {
                            if item.is_none() {
                                break;
                            }
                            if let Some(v) = item.value {
                                if let Ok(s) = String::from_utf8(v.to_vec()) {
                                    res.push((id, s));
                                    id += 1;
                                }
                            }
                        }
                    }
                    res.reverse();
                    res.into_iter().unique_by(|e| e.1.clone()).collect()
                })
            })
            .and_then(|res| tx.rollback().map_err(Error::DBTxError).map(|_| res))
    })
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
    if input.is_empty() {
        let entries = &state.history[..state.config.max_entries];
        entries
            .into_iter()
            .map(|(id, entry)| {
                let mut title = entry.clone().replace('\n', " ");
                title.truncate(100);
                Match {
                    title: title.trim().into(),
                    description: ROption::RNone,
                    use_pango: false,
                    icon: ROption::RNone,
                    id: ROption::RSome(*id),
                }
            })
            .collect()
    } else {
        let matcher = fuzzy_matcher::skim::SkimMatcherV2::default().smart_case();
        let mut entries = state
            .history
            .iter()
            .filter_map(|(id, e)| {
                let score = matcher.fuzzy_match(e.as_str(), &input).unwrap_or(0);
                if score > 0 {
                    Some((id, e, score))
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
                let mut title = entry.clone().replace('\n', " ");
                title.truncate(100);
                Match {
                    title: title.trim().into(),
                    description: ROption::RNone,
                    use_pango: false,
                    icon: ROption::RNone,
                    id: ROption::RSome(*id),
                }
            })
            .collect()
    }
}

#[handler]
fn handler(selection: Match, state: &State) -> HandleResult {
    let entry = state
        .history
        .iter()
        .find_map(|(id, entry)| {
            if *id == selection.id.unwrap() {
                Some(entry)
            } else {
                None
            }
        })
        .unwrap();
    HandleResult::Copy(entry.as_bytes().into())
}
