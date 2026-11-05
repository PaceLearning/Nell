# Nell

Nell is grep for AI agents. It searches files the way grep does, and it remembers every file it reads, so that many agents searching the same code do not each read and hold the same files.

## How an agent uses it

Search by text, the same as grep. The output is the same shape as grep's, `path:line:text`:

```
nell "fn validate_token" src/
nell -i "validate" src/
nell -n "TODO" .
nell -l "validate" src/
nell -c "validate" src/
nell -w "token" src/
nell -C 2 "validate" src/
nell -iw "token" src/
```

The grep flags work as they do in grep, and short flags can be combined. `-n` changes nothing, because Nell always prints the line number. `-l` prints each matching file once, `-c` prints `path:count`, `-w` matches whole words only, and `-C 2` prints two lines around each match as `path-line-text`, with `--` between groups that are apart.

A search prints at most 50 matches, grouped by file. Context lines from `-C` do not count. Files where the match is defined come first, then other source files, then tests. When lines were left out, the last line says how many, such as `312 more matches in 41 files, --all prints them`. Set the bound, or print everything:

```
nell --limit 200 "validate" src/
nell --all "validate" src/
```

Search by meaning. Nell answers from the files it has already read:

```
nell --meaning "where a token is checked before a request is served" src/
```

It prints the 10 closest places, best first, as `path:line:text`. `--limit` and `--all` change the count. It answers only from what the store holds, so run `nell init` first. A place is ranked by how close its vector is to the question's, both from a small local model. When the store has no vectors yet, or the model cannot be loaded, Nell says so on stderr and ranks by the words a place shares with the question instead, with names such as `ValidateToken` split into `validate` and `token`.

See what Nell holds:

```
nell status
```

Index a repository once, so that every later search, from any agent, reads from the store and can search by meaning. This is the one slow command, tens of seconds for a large repository. The first run on a machine also downloads the model, about 90 MB, into Nell's data folder; after that nothing needs the network:

```
nell init
```

Bring the store up to date after files have changed, such as after a pull. It does only the files that changed or are new, and gives their chunks vectors:

```
nell update
```

Both print one line, such as `312 files, 1840 chunks, 1840 vectors, 12.4 seconds`.

## What happens on a search

1. Nell opens the store, one file shared by every `nell` process on the machine.
2. For each file the search would read, Nell looks in the store first. A file is read from disk once. The next process that searches it reads it from the store instead.
3. A file that is not in the store, or has changed on disk since it was stored, is read and stored. A text search never loads the model, so the new chunks have no vectors until `nell update` or the next search by meaning gives them one.
4. The matches print the same way grep prints them.

A search by meaning loads the model once, for the question. While it has it open it gives a vector to the chunks under the searched paths that have none, up to 300 of them. With more than that it answers with the vectors it has and prints one line saying to run `nell update`. Then it compares the question with the vectors and prints the closest places, as `path:line:text`.

## The store

The store is one file that every `nell` process maps into memory. It holds, per file: the path, the size and time it had on disk, the text, and one vector per chunk of the text. Because every process maps the same file, the operating system keeps one copy of it in memory no matter how many agents are searching.

The store is Nell's own format. Nothing has to be installed or running for it to work.

The vectors come from all-MiniLM-L6-v2, a small sentence model run inside Nell through the ONNX runtime. Each vector is 384 numbers, kept as 8 bit integers with one scale per vector.

## Not decided yet

- Where the store file lives.
