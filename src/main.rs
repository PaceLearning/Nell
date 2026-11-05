mod files;
mod meaning;
mod model;
mod record;
mod search;
mod store;
mod vector;
mod words;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use files::Kept;
use model::Model;
use search::{Match, Search};
use store::Store;

const USAGE: &str = "\
Usage: nell [-inlcw] [-C <lines>] [--all | --limit <lines>] <pattern> [path...]
       nell --meaning <question> [--all | --limit <lines>] [path...]
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

A search by meaning answers from what nell init stored, and prints the
10 closest places, best first. --limit and --all change the count.
";

/// How many lines a text search prints when neither --all nor --limit is given.
const DEFAULT_LIMIT: usize = 50;

/// How many places a search by meaning prints when neither --all nor --limit is given.
const MEANING_LIMIT: usize = 10;

/// The most chunks without a vector that a search by meaning embeds before it answers. With more than this it
/// answers with the vectors it has and says to run nell update.
const MEANING_EMBED_LIMIT: usize = 300;

fn main() -> ExitCode {
    let mut ignore_case = false;
    let mut whole_word = false;
    let mut files_only = false;
    let mut count_only = false;
    let mut context: Option<usize> = None;
    // Nothing when neither --all nor --limit was given, so each kind of search can use its own default.
    let mut limit: Option<Option<usize>> = None;
    let mut meaning: Option<String> = None;
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
            limit = Some(None);
            continue;
        }
        if argument == "--limit" {
            let Some(Ok(lines)) = arguments.next().map(|value| value.parse::<usize>()) else {
                eprintln!("nell: --limit needs a whole number of lines");
                return ExitCode::from(2);
            };
            limit = Some(Some(lines));
            continue;
        }
        if argument == "--meaning" {
            let Some(question) = arguments.next() else {
                eprintln!("nell: --meaning needs a question");
                return ExitCode::from(2);
            };
            meaning = Some(question);
            continue;
        }
        words.push(argument);
    }
    if let Some(question) = meaning {
        let mut paths = words;
        if paths.is_empty() {
            paths.push(".".to_string());
        }
        let opened = store::root_of(Path::new(&paths[0])).and_then(|root| Store::open(&root));
        let mut store = match opened {
            Ok(store) => store,
            Err(error) => {
                eprintln!("nell: {error}");
                return ExitCode::from(2);
            }
        };
        // Only what the store already holds is searched. A held file that changed on disk is stored again
        // first, so the lines printed are the lines on disk; its new chunks have no vectors yet.
        let mut shown: Vec<PathBuf> = Vec::new();
        let mut kept: Vec<Kept> = Vec::new();
        for entry in files::walk(Path::new(&paths[0]), &paths[1..]) {
            let Ok(entry) = entry else { continue };
            let Some(key) = files::key(&store, entry.path()) else { continue };
            if !store.knows(&key) {
                continue;
            }
            let file = match files::keep(&mut store, &entry) {
                Ok(file) => file,
                Err(error) => {
                    eprintln!("nell: {error}");
                    return ExitCode::from(2);
                }
            };
            let Some(file) = file else { continue };
            shown.push(entry.path().to_path_buf());
            kept.push(file);
        }
        let mut any_chunk = false;
        let mut missing_files = 0;
        let mut missing_chunks = 0;
        for file in &kept {
            let Some(record) = store.held(&file.key, file.size, file.modified) else { continue };
            let missing = record.chunk_count().saturating_sub(record.vector_count());
            if record.chunk_count() > 0 {
                any_chunk = true;
            }
            if missing > 0 {
                missing_files += 1;
                missing_chunks += missing;
            }
        }
        if !any_chunk {
            eprintln!("nell: nothing indexed yet, run nell init");
            return ExitCode::from(2);
        }
        let mut model: Option<Model> = None;
        let has_vectors = store.status().vectors > 0;
        if !has_vectors {
            eprintln!("nell: the store has no vectors yet, run nell init; answering by shared words");
        }
        if has_vectors {
            match Model::load(store.folder()) {
                Ok(loaded) => model = Some(loaded),
                Err(error) => eprintln!("nell: {error}; answering by shared words"),
            }
        }
        let mut asked: Option<Vec<f32>> = None;
        if let Some(model) = &mut model {
            match model.embed(std::slice::from_ref(&question)) {
                Ok(mut vectors) => asked = vectors.pop(),
                Err(error) => eprintln!("nell: {error}; answering by shared words"),
            }
        }
        if asked.is_some() && missing_chunks > MEANING_EMBED_LIMIT {
            eprintln!("nell: {missing_files} files changed since the store was indexed, run nell update");
        }
        if asked.is_some()
            && missing_chunks > 0
            && missing_chunks <= MEANING_EMBED_LIMIT
            && let Some(model) = &mut model
            && let Err(error) = files::embed(&mut store, model, &kept)
        {
            eprintln!("nell: {error}");
            return ExitCode::from(2);
        }
        let mut chunks = Vec::new();
        for (path, file) in shown.iter().zip(&kept) {
            let Some(record) = store.held(&file.key, file.size, file.modified) else { continue };
            let lines: Vec<&str> = record.lines().collect();
            for (index, (first, last)) in record.chunks().into_iter().enumerate() {
                let Some(text) = lines.get(first.saturating_sub(1)..last) else { continue };
                let lines = text.iter().map(|line| line.to_string()).collect();
                let vector = record.vector(index);
                chunks.push(meaning::Chunk { path: path.clone(), first, lines, vector });
            }
        }
        if asked.is_some() && !chunks.iter().any(|chunk| chunk.vector.is_some()) {
            eprintln!("nell: no vectors under these paths yet; answering by shared words");
            asked = None;
        }
        let ranked = match &asked {
            Some(asked) => meaning::rank_by_vectors(&question, asked, chunks),
            None => meaning::rank(&question, chunks),
        };
        if ranked.is_empty() {
            return ExitCode::from(1);
        }
        let count = limit.unwrap_or(Some(MEANING_LIMIT)).unwrap_or(ranked.len());
        for item in ranked.iter().take(count) {
            let text = item.chunk.lines.get(item.line - item.chunk.first).map_or("", String::as_str);
            println!("{}:{}:{}", item.chunk.path.display(), item.line, text);
        }
        return ExitCode::SUCCESS;
    }
    let limit = limit.unwrap_or(Some(DEFAULT_LIMIT));
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
        println!("vectors: {}", status.vectors);
        println!("bytes: {}", status.bytes);
        println!("dead bytes: {}", status.dead_bytes);
        return ExitCode::SUCCESS;
    }
    if words.len() == 1 && (words[0] == "init" || words[0] == "update") {
        let started = Instant::now();
        let opened = store::root_of(Path::new(".")).and_then(|root| Store::open(&root));
        let mut store = match opened {
            Ok(store) => store,
            Err(error) => {
                eprintln!("nell: {error}");
                return ExitCode::from(2);
            }
        };
        // init starts the store over, so every file is new to it. update keeps what has not changed.
        if words[0] == "init" && let Err(error) = store.clear() {
            eprintln!("nell: {error}");
            return ExitCode::from(2);
        }
        let root = store.root().to_path_buf();
        let mut kept: Vec<Kept> = Vec::new();
        let mut file_count = 0;
        let mut chunk_count = 0;
        for entry in files::walk(&root, &[]) {
            let Ok(entry) = entry else { continue };
            let file = match files::keep(&mut store, &entry) {
                Ok(file) => file,
                Err(error) => {
                    eprintln!("nell: {error}");
                    return ExitCode::from(2);
                }
            };
            let Some(file) = file else { continue };
            let Some(record) = store.held(&file.key, file.size, file.modified) else { continue };
            if file.stored {
                file_count += 1;
                chunk_count += record.chunk_count();
            }
            kept.push(file);
        }
        // The model is loaded once, after every file is stored, and gives a vector to each chunk without one:
        // the chunks stored just now and any that a text search left without.
        let mut vector_count = 0;
        match Model::load(store.folder()) {
            Ok(mut model) => match files::embed(&mut store, &mut model, &kept) {
                Ok(made) => vector_count = made,
                Err(error) => {
                    eprintln!("nell: {error}");
                    return ExitCode::from(2);
                }
            },
            Err(error) => eprintln!("nell: no vectors made, {error}; run nell update once the model loads"),
        }
        let seconds = started.elapsed().as_secs_f64();
        println!("{file_count} files, {chunk_count} chunks, {vector_count} vectors, {seconds:.1} seconds");
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
