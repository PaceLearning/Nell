mod files;
mod record;
mod search;
mod store;

use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use search::Search;
use store::Store;

const USAGE: &str = "\
Usage: nell [-i] [--all | --limit <lines>] <pattern> [path...]
       nell --meaning <question> [path...]
       nell init | update | status

Search files the way grep does. Every file read is kept in a shared store,
so the next search reads it from there, and can search it by meaning.

A search prints at most 50 lines, definitions first and tests last.
--limit sets that bound and --all prints every line.
";

/// How many lines a text search prints when neither --all nor --limit is given.
const DEFAULT_LIMIT: usize = 50;

fn main() -> ExitCode {
    let mut ignore_case = false;
    let mut limit = Some(DEFAULT_LIMIT);
    let mut words: Vec<String> = Vec::new();
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "-i" {
            ignore_case = true;
            continue;
        }
        if argument == "--all" {
            limit = None;
            continue;
        }
        if argument == "--limit" {
            let Some(Ok(lines)) = arguments.next().map(|value| value.parse::<usize>()) else {
                eprintln!("nell: --limit needs a whole number of lines");
                return ExitCode::from(2);
            };
            limit = Some(lines);
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
        println!("chunks: {}", status.chunks);
        println!("bytes: {}", status.bytes);
        println!("dead bytes: {}", status.dead_bytes);
        return ExitCode::SUCCESS;
    }
    if words.len() == 1 && words[0] == "init" {
        let started = Instant::now();
        let opened = store::root_of(Path::new(".")).and_then(|root| Store::open(&root));
        let mut store = match opened {
            Ok(store) => store,
            Err(error) => {
                eprintln!("nell: {error}");
                return ExitCode::from(2);
            }
        };
        if let Err(error) = store.clear() {
            eprintln!("nell: {error}");
            return ExitCode::from(2);
        }
        let root = store.root().to_path_buf();
        let mut file_count = 0;
        let mut chunk_count = 0;
        for entry in files::walk(&root, &[]) {
            let Ok(entry) = entry else { continue };
            let kept = match files::keep(&mut store, &entry) {
                Ok(kept) => kept,
                Err(error) => {
                    eprintln!("nell: {error}");
                    return ExitCode::from(2);
                }
            };
            let Some(kept) = kept else { continue };
            let Some(record) = store.held(&kept.key, kept.size, kept.modified) else { continue };
            file_count += 1;
            chunk_count += record.chunk_count();
        }
        let seconds = started.elapsed().as_secs_f64();
        println!("{file_count} files, {chunk_count} chunks, {seconds:.1} seconds");
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
    let mut printed = 0;
    let mut more_matches = 0;
    let mut more_files = 0;
    for group in &found {
        let room = limit.map_or(group.matches.len(), |limit| limit.saturating_sub(printed));
        let shown = group.matches.len().min(room);
        for item in &group.matches[..shown] {
            println!("{}:{}:{}", group.path.display(), item.line, item.text);
        }
        printed += shown;
        let cut = group.matches.len() - shown;
        if cut > 0 {
            more_matches += cut;
            more_files += 1;
        }
    }
    if more_matches > 0 {
        println!("{more_matches} more matches in {more_files} files, --all prints them");
    }
    if found.is_empty() {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
