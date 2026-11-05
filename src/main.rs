const USAGE: &str = "\
Usage: nell [options] <pattern> [path...]
       nell --meaning <question> [path...]
       nell status

Search files the way grep does. Every file read is kept in a shared store,
so the next search reads it from there, and can search it by meaning.
";

fn main() {
    eprint!("{USAGE}");
    std::process::exit(2);
}
