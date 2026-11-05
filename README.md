# Nell

Nell is grep for AI agents. It searches files the way grep does, and it remembers every file it reads, so that many agents searching the same code do not each read and hold the same files.

## How an agent uses it

Search by text, the same as grep. The output is the same shape as grep's, `path:line:text`:

```
nell "fn validate_token" src/
nell -i "validate" src/
nell -n "TODO" .
```

Search by meaning. Nell answers from the files it has already read:

```
nell --meaning "where a token is checked before a request is served" src/
```

See what Nell holds:

```
nell status
```

## What happens on a search

1. Nell opens the store, one file shared by every `nell` process on the machine.
2. For each file the search would read, Nell looks in the store first. A file is read from disk once. The next process that searches it reads it from the store instead.
3. A file that is not in the store, or has changed on disk since it was stored, is read, stored, and given a vector so that it can be found by meaning.
4. The matches print the same way grep prints them.

A search by meaning compares the question with the vectors in the store and prints the closest places, as `path:line:text`.

## The store

The store is one file that every `nell` process maps into memory. It holds, per file: the path, the size and time it had on disk, the text, and one vector per chunk of the text. Because every process maps the same file, the operating system keeps one copy of it in memory no matter how many agents are searching.

The store is Nell's own format. Nothing has to be installed or running for it to work.

## Not decided yet

- Where the store file lives.
- Which model gives a file its vector.
