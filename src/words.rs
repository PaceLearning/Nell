//! Splitting text into the words a search by meaning compares.

/// Splits `text` into lowercase words. Anything that is not a letter or a digit ends a word, and so does a change
/// from lower to upper case inside an identifier, so `ValidateToken` and `validate_token` both give `validate`
/// and `token`. A run of capitals stays one word, so `HTTPServer` gives `http` and `server`.
pub fn split(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut words = Vec::new();
    let mut word = String::new();
    for (at, &letter) in chars.iter().enumerate() {
        let separator = !letter.is_alphanumeric();
        if separator && !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        if separator {
            continue;
        }
        let previous = at.checked_sub(1).and_then(|before| chars.get(before)).copied();
        let next = chars.get(at + 1).copied();
        let after_lower = previous.is_some_and(|before| before.is_lowercase() || before.is_numeric());
        let capitals_end = previous.is_some_and(char::is_uppercase) && next.is_some_and(char::is_lowercase);
        if letter.is_uppercase() && (after_lower || capitals_end) && !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        word.extend(letter.to_lowercase());
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}
