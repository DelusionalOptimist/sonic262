use color_eyre::eyre::Result;
// TODO use a color library with no runtime dependency like yansi
use colored::Colorize;
use dashmap::DashMap;
use rayon::prelude::*;
use serde_yaml::Value::Sequence;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitStatus;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tempfile::NamedTempFile;
use walkdir::WalkDir;

#[derive(Debug)]
pub struct Diagnostics {
    total_files: AtomicU32,
    run: AtomicU32,
    passed: AtomicU32,
    failed: AtomicU32,
    no_frontmatter: AtomicU32,
}

impl Diagnostics {
    fn new() -> Self {
        Self {
            total_files: AtomicU32::new(0),
            run: AtomicU32::new(0),
            passed: AtomicU32::new(0),
            failed: AtomicU32::new(0),
            no_frontmatter: AtomicU32::new(0),
        }
    }
}

// TODO get rid of this function. This is just a match wrapper around generate_final_file_to_test
pub fn generate_and_run(
    file_to_test: &PathBuf,
    include_contents: String,
    diagnostics: Arc<Diagnostics>,
) {
    match generate_final_file_to_test(file_to_test, include_contents) {
        Ok(file_to_run) => match spawn_node_process(file_to_run) {
            Ok(exit_status) => {
                if exit_status.success() {
                    diagnostics.run.fetch_add(1, Ordering::Relaxed);
                    diagnostics.passed.fetch_add(1, Ordering::Relaxed);
                    println!("{} {:?}", "Great Success".green(), file_to_test);
                } else {
                    diagnostics.run.fetch_add(1, Ordering::Relaxed);
                    diagnostics.failed.fetch_add(1, Ordering::Relaxed);
                    eprintln!("{} {:?}", "FAIL".red(), file_to_test);
                }
            }
            Err(e) => eprintln!("Failed to execute the file_to_test | Error: {:?}", e),
        },
        Err(e) => eprintln!(
            "Couldn't generate file: {:?} to test | Err: {}",
            file_to_test, e
        ),
    }
}

fn walk(root_path: PathBuf) -> Result<Vec<PathBuf>> {
    let mut final_paths: Vec<PathBuf> = vec![];
    for entry in WalkDir::new(root_path) {
        // FIXME possible unecessary clone
        let entry_clone = entry?.clone();
        if entry_clone.file_type().is_file() {
            final_paths.push(entry_clone.into_path());
        } else {
        }
    }
    Ok(final_paths)
}

pub fn extract_frontmatter(file_to_test: &PathBuf) -> Option<String> {
    // FIXME remove unwrap
    let file_contents = fs::read_to_string(file_to_test).unwrap();
    let yaml_start = file_contents.find("/*---");
    if let Some(start) = yaml_start {
        let yaml_end = file_contents.find("---*/");
        if let Some(end) = yaml_end {
            // TODO remove unwrap here
            Some(file_contents.get(start + 5..end).unwrap().to_string())
        } else {
            eprintln!("This file has an invalid frontmatter");
            None
        }
    } else {
        eprintln!("frontmatter not found in file: {:?}", file_to_test);
        None
    }
}

pub fn get_serde_value(frontmatter_str: &str) -> serde_yaml::Result<serde_yaml::Value> {
    serde_yaml::from_str(frontmatter_str)
}

pub fn get_contents(
    includes_map: &DashMap<String, String>,
    includes_value: &serde_yaml::Value,
    include_path_root: &PathBuf,
) -> Result<String> {
    let mut includes: Vec<String> = vec!["assert".to_string(), "sta".to_string()];
    includes.append(&mut serde_yaml::from_value(includes_value.clone())?);
    let includes_clone = includes.clone();
    for include in includes {
        let include_clone = include.clone();
        if !(includes_map.contains_key(&include)) {
            // read &include and store it in hashmap
            includes_map.insert(
                include,
                fs::read_to_string(include_path_root.join(include_clone)).unwrap(),
            );
        }
    }
    let mut contents = String::new();
    for include in includes_clone {
        match includes_map.get(&include) {
            Some(includes_value) => {
                contents.push_str(&*includes_value);
                contents.push('\n');
            }
            None => eprintln!("This should not have happened :|"),
        }
    }
    Ok(contents)
}

// TODO combine this and get_contents()
fn generate_final_file_to_test(
    file_to_test: &PathBuf,
    mut contents: String,
) -> Result<NamedTempFile> {
    let file_to_test_contents = fs::read_to_string(file_to_test)?;
    contents.push_str(&file_to_test_contents);
    let mut file = tempfile::Builder::new().suffix(".js").tempfile()?;
    writeln!(file, "{}", contents)?;
    Ok(file)
}

pub fn spawn_node_process(file: NamedTempFile) -> std::io::Result<ExitStatus> {
    // TODO .status() waits for the command to execute
    // FIXME currently shows all the errors if node is not able to run the file
    Command::new("node").arg(file.path()).status()
}

pub fn run_all(test_path: PathBuf, include_path: PathBuf) -> Result<()> {
    let includes_map: DashMap<String, String> = DashMap::new();
    includes_map.insert(
        "assert".to_string(),
        fs::read_to_string(include_path.join("assert.js"))?,
    );
    includes_map.insert(
        "sta".to_string(),
        fs::read_to_string(include_path.join("sta.js"))?,
    );

    let files_to_test = walk(test_path)?;
    let diagnostics = Arc::new(Diagnostics::new());
    files_to_test.into_par_iter().for_each(|file| {
        let frontmatter = extract_frontmatter(&file);
        match frontmatter {
            Some(f) => match get_serde_value(&f) {
                Ok(frontmatter_value) => match frontmatter_value.get("includes") {
                    Some(includes_value) => {
                        match get_contents(&includes_map, includes_value, &include_path) {
                            Ok(include_contents) => {
                                generate_and_run(&file, include_contents, diagnostics.clone())
                            }
                            Err(e) => eprintln!("Not able to find {:?} | Err: {}", file, e),
                        }
                    }
                    None => {
                        match get_contents(&includes_map, &Sequence([].to_vec()), &include_path) {
                            Ok(include_contents) => {
                                generate_and_run(&file, include_contents, diagnostics.clone())
                            }
                            Err(e) => eprintln!("Not able to find {:?} | Err: {}", file, e),
                        }
                    }
                },
                Err(e) => eprintln!("Could not get serde value from frontmatter | Err: {}", e),
            },
            None => {
                diagnostics.no_frontmatter.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
    println!(
        "TOTAL: {}, FAILED: {}, PASSED: {}, NO FRONTMATTER: {}",
        diagnostics.run.load(Ordering::Relaxed).to_string().yellow(),
        diagnostics.failed.load(Ordering::Relaxed).to_string().red(),
        diagnostics
            .passed
            .load(Ordering::Relaxed)
            .to_string()
            .green(),
        diagnostics
            .no_frontmatter
            .load(Ordering::Relaxed)
            .to_string()
            .cyan(),
    );
    Ok(())
}

#[cfg(test)]
mod test {
    use super::Diagnostics;
    use static_assertions::assert_impl_all;
    use std::marker::Send;
    use std::marker::Sync;

    #[test]
    fn test() {
        assert_impl_all!(Diagnostics: Send, Sync);
    }
}
