# Better than grep

Nothing in this folder is built. This note says what grep does for an agent today, where that falls short, and what Nell would do instead.

## What exists

grep, and ripgrep, which is what Claude Code runs behind its Grep tool. ripgrep walks a tree in parallel, skips what `.gitignore` lists, matches a regular expression against each line, and prints `path:line:text`. It searches a whole repository in well under a second.

Every run starts from nothing. It opens every file again, remembers nothing from the last run, and matches characters. The operating system keeps the bytes of a file that one process read, so the next process gets them from memory. Reading is not the slow part. What is lost between runs is the work done on the bytes, and grep does almost none, so it has nothing to lose. Nell will do more work per file, so it has to do it once.

## What an agent does with it today

An agent wants to know where a token is checked before a request is served in Pace. It runs:

```
grep -rn "token" src/
```

and gets hundreds of lines: tests, comments, client code, the word inside other words. It tries `ValidateToken`, then `Validate.*Token`, then `IsValid`. Four runs, a few hundred lines pulled into its context, then it opens three files to find the one. Twenty agents on the same repository each do the same. The cost is tokens and turns, not seconds.

The gaps, in order of how much they cost:

1. **Text, not meaning.** The agent has to guess the word the author used.
2. **Every mention alike.** A definition, a call, a comment and a test all print the same way, in file order.
3. **Everything prints.** The output is not shaped for a reader with a token budget. Three hundred lines is a bad answer even when the right one is in it.
4. **Nothing is remembered.** What one agent worked out about a file, the next one works out again.

## What Nell keeps from grep

The command shape, the output line, the exit codes (0 for a match, 1 for none, 2 for an error), and the flags that matter: `-i`, `-n`, `-l`, `-c`, `-C`, `-w`. `.gitignore` is respected the way ripgrep does it. An agent that knows grep knows nell, and a tool that wraps grep can wrap nell with no change.

## Where Nell is better

1. **Search by meaning.** `nell --meaning "where a token is checked before a request is served" src/` prints the closest places as `path:line:text`, nearest first. Each file is split into chunks, each chunk has a vector, and the question is compared with them.
2. **Search by symbol.** `nell --symbol ValidateToken src/` prints where the name is defined first, then where it is used. A text match on `ValidateToken` prints both in file order and leaves the sorting to the reader.
3. **Output for a token budget.** By default Nell prints a bounded number of lines, grouped by file, with one closing line such as `312 more matches in 41 files, --all prints them`. Order within the bound: definitions, then source, then tests. The agent gets the best lines first and decides whether to ask for more.
4. **Read once, index once.** The store. A file's text, line starts, chunks and vectors are computed the first time any process reads it, and every later process, from any agent, reads them from the store. A file is indexed again only when its size or time on disk has changed.

## The store

Two shapes.

**Option A, Nell's own file, mapped into memory.** A header, then records appended one after another. One record per file version: the path, the size and time it had on disk, the text, the line starts, and one vector per chunk. A small index maps a path to its latest record. A writer takes an operating system file lock, appends, then advances the length in the header. A reader maps the file and trusts only what is inside that length, so it never sees a half written record. When dead records are more than half the file, the next writer copies the live ones into a fresh file and swaps it in.

**Option B, an embedded database in one file.** redb, or SQLite through rusqlite. Locking, crash safety and compaction come with it. It costs a dependency, and the vectors sit as blobs inside pages rather than as arrays a process can map and scan straight through.

Recommendation: A. The data is simple, it is only ever appended, and the one real risk, a crash mid write, is covered by the rule that a record exists only once the header's length covers it. Readers of a mapped file share one copy of its pages, so a hundred processes hold the store once.

## Many processes at once

Every `nell` is a short process, and there may be a hundred of them. Readers only map the file and never block each other. Writers hold the lock only for the append. When two agents hit the same new file at the same moment, the second one takes the lock, finds the record the first one just wrote, and skips. Each process adds only its own working buffers to what the machine holds.

## The vectors

Three shapes.

**Option A, a local model inside nell.** A small sentence model run through ONNX, through the fastembed crate. The model is downloaded once, tens of megabytes, and nothing needs the network after that. The binary carries the ONNX runtime.

**Option B, a hosted model.** A request per new file to an embeddings service. It needs a key and a network, and a grep that fails without a network is a worse grep.

**Option C, no model.** Rank chunks by the words they share with the question, with identifiers split on case and underscores, so `ValidateToken` matches `token` and `validate`. Offline and tiny, but it is better word matching, not meaning.

Recommendation: A. Build the chunks and the store first, then plug the model in last, so that every other piece works before the heaviest dependency lands.

## What the vectors cost, and where it is paid

Vectors cost more than grep, in three places. The model is 25 to 90 MB of weights plus the ONNX runtime, paid by every process that loads it. Computing one chunk's vector is a few milliseconds of CPU, paid once per chunk per file version. One chunk's vector is about 400 bytes stored as 8 bit integers, paid once in the store and shared by every process that maps it. Comparing a question with ten thousand chunks is under a millisecond.

The one that matters is the model load. A `nell` process lives for a fraction of a second. If each one loaded the model to embed the three files it touched, a hundred agents would mean a hundred model loads, far worse than grep. So the model is loaded in as few places as possible:

- **`nell init`** does the first full pass in one process. It walks the repository, respecting `.gitignore`, reads every text file into the store, splits it into chunks, loads the model once, and gives every chunk its vector. Tens of seconds for a repository the size of Pace, paid once per machine. Running it on a repository that already has a store starts that store over.
- **`nell update`** brings a store up to date after files have changed. It loads the model once and does only the files whose size or time on disk differ from what the store holds, and the files the store has never seen. It is what to run after a pull or a long edit, instead of `init` again.
- **A text search never loads the model.** It maps the store, reads the text, matches, prints. When it reads a file that has changed since it was stored, it stores the new text and chunks and marks their vectors as missing. Its only cost over grep is mapping a file the operating system already holds.
- **A `--meaning` search loads the model once, for the question.** Since it has the model open, before it answers it embeds whatever chunks are missing a vector. A handful of edited files costs milliseconds. If there are more than a few hundred such chunks it answers with what it has and prints one line saying to run `nell update`.

With those rules, a plain `nell "pattern"` is grep plus a memory map, a hundred agents searching by text pay nothing for the vectors, and the model loads only in `init`, `update` and `--meaning`. Nothing runs in the background.

## Where the store lives

**Option A, in the repository**, at `.nell/store` next to `.git`, with a line in that repository's `.gitignore`.

**Option B, with the user**, under the user's local data folder, one store per repository path.

Recommendation: B. Nothing is added to any repository, and there is one place to clear.

## What has to change elsewhere

Agents have to run `nell` where they run grep. For Claude Code that is a line in `CLAUDE.md` or a skill. Nothing in Pace changes.

## Order of work

1. Text search with grep's shape, exit codes and `.gitignore`, reading from disk, no store.
2. The store: records, the lock, read once.
3. `nell init`, filling the store with text and chunks for the whole repository, no vectors yet.
4. The output bound and the ordering.
5. `--meaning`, scored by shared words first.
6. The local model: vectors in `init`, `nell update`, and the question in `--meaning`, replacing the word scorer.
7. `--symbol`.
