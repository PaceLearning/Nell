mod files;
mod record;
mod search;
mod store;

use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use search::{Match, Search};
use store::Store;

const USAGE: &str = "\
Usage: nell [-inlcw] [-C <lines>] [--all | --limit <lines>] <pattern> [path...]
       nell --meaning <question> [path...]
       nell init | update | status

Search files the way grep does. Every file read is kept in a shared store,
so the next search reads it from there, and can search it by meaning.

  -i  ignore case
  -n  print line numbers, which nell always does
  -l  print only the path of each file that matches
  -c  print each matching file's path and how many lines match
  -w  match whole words only
  -C  print this many lines of context around each match

A search prints at most 50 matches, definitions first and tests last.
--limit sets that bound and --all prints every match.
";

/// How many lines a text search prints when neither --all nor --limit is given.
const DEFAULT_LIMIT: usize = 50;

fn main() -> ExitCode {
    let mut ignore_case = false;
    let mut whole_word = false;
    let mut files_only = false;
    let mut count_only = false;
    let mut context: Option<usize> = None;
    let mut limit = Some(DEFAULT_LIMIT);
    let mut words: Vec<String> = Vec::new();
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--" {
            words.extend(arguments.by_ref());
            continue;
        }
        if argument.len() > 1 && argument.starts_with('-') && !argument.starts_with("--") {
            // Short flags, one or several together such as -in, the way grep takes them.
            let flags: Vec<char> = argument[1..].chars().collect();
            let mut at = 0;
            while at < flags.len() {
                let flag = flags[at];
                at += 1;
                match flag {
                    'i' => ignore_case = true,
                    'n' => {}
                    'l' => files_only = true,
                    'c' => count_only = true,
                    'w' => whole_word = true,
                    'C' => {
                        let attached: String = flags[at..].iter().collect();
                        at = flags.len();
                        let mut value = Some(attached.clone());
                        if attached.is_empty() {
                            value = arguments.next();
                        }
                        let Some(Ok(lines)) = value.map(|value| value.parse::<usize>()) else {
                            eprintln!("nell: -C needs a whole number of lines");
                            return ExitCode::from(2);
                        };
                        context = Some(lines);
                    }
                    _ => {
                        eprintln!("nell: unknown flag -{flag}");
                        return ExitCode::from(2);
                    }
                }
            }
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
    let search = match Search::create(&pattern, paths.clone(), ignore_case, whole_word, context.unwrap_or(0)) {
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
    if found.is_empty() {
        return ExitCode::from(1);
    }
    if files_only {
        for group in &found {
            println!("{}", group.path.display());
        }
        return ExitCode::SUCCESS;
    }
    if count_only {
        for group in &found {
            println!("{}:{}", group.path.display(), group.matches.len());
        }
        return ExitCode::SUCCESS;
    }
    let mut printed = 0;
    let mut more_matches = 0;
    let mut more_files = 0;
    // The last line printed with context on, as the group it came from and its line number, to know where a
    // `--` goes.
    let mut last: Option<(usize, usize)> = None;
    for (number, group) in found.iter().enumerate() {
        let room = limit.map_or(group.matches.len(), |limit| limit.saturating_sub(printed));
        let shown = group.matches.len().min(room);
        let path = group.path.display();
        if let Some(around) = context {
            let kept = &group.matches[..shown];
            // Context stops before the first match the bound left out, so that match is not printed as context.
            let stop = group.matches.get(shown).map_or(usize::MAX, |item| item.line);
            let mut lines: Vec<(&Match, char)> = kept.iter().map(|item| (item, ':')).collect();
            for item in &group.context {
                if item.line >= stop {
                    continue;
                }
                let after = kept.partition_point(|shown| shown.line < item.line);
                let near_after = kept.get(after).is_some_and(|shown| shown.line - item.line <= around);
                let near_before = after > 0 && item.line - kept[after - 1].line <= around;
                if near_after || near_before {
                    lines.push((item, '-'));
                }
            }
            lines.sort_by_key(|(item, _)| item.line);
            for (item, mark) in lines {
                let next = last.is_some_and(|(at, line)| at == number && line + 1 == item.line);
                if last.is_some() && !next {
                    println!("--");
                }
                println!("{path}{mark}{}{mark}{}", item.line, item.text);
                last = Some((number, item.line));
            }
        }
        if context.is_none() {
            for item in &group.matches[..shown] {
                println!("{path}:{}:{}", item.line, item.text);
            }
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
    ExitCode::SUCCESS
}
