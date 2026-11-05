mod record;
mod search;
mod store;

use std::path::Path;
use std::process::ExitCode;

use search::Search;
use store::Store;

const USAGE: &str = "\
Usage: nell [-i] <pattern> [path...]
       nell --meaning <question> [path...]
       nell init | update | status

Search files the way grep does. Every file read is kept in a shared store,
so the next search reads it from there, and can search it by meaning.
";

fn main() -> ExitCode {
    let mut ignore_case = false;
    let mut words: Vec<String> = Vec::new();
    for argument in std::env::args().skip(1) {
        if argument == "-i" {
            ignore_case = true;
            continue;
        }
        words.push(argument);
    }
    if words.is_empty() {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    }
    if words.len() == 1 && words[0] == "status" {
        let opened = store::root_of(Path::new(".")).and_then(|root| Store::open(&root));
        let store = match opened {
            Ok(store) => store,
            Err(error) => {
                eprintln!("nell: {error}");
                return ExitCode::from(2);
            }
        };
        let status = store.status();
        println!("store: {}", status.path.display());
        println!("files: {}", status.files);
        println!("bytes: {}", status.bytes);
        println!("dead bytes: {}", status.dead_bytes);
        return ExitCode::SUCCESS;
    }
    let pattern = words.remove(0);
    let mut paths = words;
    if paths.is_empty() {
        paths.push(".".to_string());
    }
    let search = match Search::create(&pattern, paths.clone(), ignore_case) {
        Ok(search) => search,
        Err(error) => {
            eprintln!("nell: {error}");
            return ExitCode::from(2);
        }
    };
    let opened = store::root_of(Path::new(&paths[0])).and_then(|root| Store::open(&root));
    let mut store = match opened {
        Ok(store) => store,
        Err(error) => {
            eprintln!("nell: {error}");
            return ExitCode::from(2);
        }
    };
    let found = match search.run(&mut store) {
        Ok(found) => found,
        Err(error) => {
            eprintln!("nell: {error}");
            return ExitCode::from(2);
        }
    };
    for item in &found {
        println!("{}:{}:{}", item.path.display(), item.line, item.text);
    }
    if found.is_empty() {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
