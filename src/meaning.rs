//! Search by meaning: chunks ranked by the cosine between their vector and the question's, or, when there are
//! no vectors to compare, by the words they share with the question.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::vector;
use crate::words;

/// How fast more of the same word stops adding to a chunk's score.
const K1: f64 = 1.2;

/// How much a long chunk is held back against a short one.
const B: f64 = 0.75;

/// One chunk of a stored file: the path to print, the number of its first line counted from 1, its lines, and
/// its vector when the store holds one.
pub struct Chunk {
    /// The path to print.
    pub path: PathBuf,
    /// The number of its first line, counted from 1.
    pub first: usize,
    /// Its lines.
    pub lines: Vec<String>,
    /// Its vector, when the store holds one.
    pub vector: Option<Vec<f32>>,
}

/// A chunk ranked against the question: the chunk, its score, and the number of its best line counted from 1.
pub struct Ranked {
    /// The chunk.
    pub chunk: Chunk,
    /// Its score against the question, higher is closer.
    pub score: f64,
    /// The number of its best line, counted from 1.
    pub line: usize,
}

/// Ranks `chunks` against `question`, best first. The question and every line are split into words with
/// `words::split`, and a chunk is scored by BM25 over the question's words, counted across all of `chunks`. A
/// chunk that shares no word with the question is left out. A chunk's best line is the one whose question words
/// weigh the most, the first one on a tie. Chunks with the same score keep the order they were given in.
pub fn rank(question: &str, chunks: Vec<Chunk>) -> Vec<Ranked> {
    let asked: HashSet<String> = words::split(question).into_iter().collect();
    let split: Vec<Vec<Vec<String>>> =
        chunks.iter().map(|chunk| chunk.lines.iter().map(|line| words::split(line)).collect()).collect();
    let mut counts: Vec<HashMap<&str, usize>> = Vec::with_capacity(chunks.len());
    let mut lengths: Vec<usize> = Vec::with_capacity(chunks.len());
    let mut holding: HashMap<&str, usize> = HashMap::new();
    for lines in &split {
        let mut count: HashMap<&str, usize> = HashMap::new();
        let mut length = 0;
        for word in lines.iter().flatten() {
            length += 1;
            if !asked.contains(word) {
                continue;
            }
            *count.entry(word.as_str()).or_insert(0) += 1;
        }
        for word in count.keys() {
            *holding.entry(word).or_insert(0) += 1;
        }
        counts.push(count);
        lengths.push(length);
    }
    let total = chunks.len() as f64;
    let average = lengths.iter().sum::<usize>() as f64 / total.max(1.0);
    let weights: HashMap<&str, f64> = holding
        .iter()
        .map(|(word, &held)| (*word, (1.0 + (total - held as f64 + 0.5) / (held as f64 + 0.5)).ln()))
        .collect();
    let mut ranked = Vec::new();
    for ((chunk, lines), (count, length)) in chunks.into_iter().zip(&split).zip(counts.iter().zip(&lengths)) {
        if count.is_empty() {
            continue;
        }
        let shrink = K1 * (1.0 - B + B * *length as f64 / average);
        let mut score = 0.0;
        for (word, &times) in count {
            let times = times as f64;
            score += weights.get(word).copied().unwrap_or(0.0) * times * (K1 + 1.0) / (times + shrink);
        }
        let mut best = 0;
        let mut best_weight = 0.0;
        for (index, line) in lines.iter().enumerate() {
            let shared: HashSet<&str> = line.iter().map(String::as_str).filter(|word| asked.contains(*word)).collect();
            let weight: f64 = shared.iter().map(|word| weights.get(word).copied().unwrap_or(0.0)).sum();
            if weight > best_weight {
                best = index;
                best_weight = weight;
            }
        }
        let line = chunk.first + best;
        ranked.push(Ranked { chunk, score, line });
    }
    ranked.sort_by(|left, right| right.score.total_cmp(&left.score));
    ranked
}

/// Ranks the chunks that have a vector against `asked`, the question's vector, best first, by the cosine
/// between the two. A chunk without a vector is left out. A chunk's best line is the one with the most of the
/// question's words, the first one on a tie, and the first line that is not blank when no line shares a word.
/// Chunks with the same score keep the order they were given in.
pub fn rank_by_vectors(question: &str, asked: &[f32], chunks: Vec<Chunk>) -> Vec<Ranked> {
    let words: HashSet<String> = words::split(question).into_iter().collect();
    let mut ranked = Vec::new();
    for chunk in chunks {
        let Some(vector) = &chunk.vector else { continue };
        let score = vector::cosine(asked, vector) as f64;
        let mut best: Option<usize> = None;
        let mut best_shared = 0;
        for (index, line) in chunk.lines.iter().enumerate() {
            let split = words::split(line);
            let shared: HashSet<&str> = split.iter().map(String::as_str).filter(|word| words.contains(*word)).collect();
            if shared.len() > best_shared {
                best = Some(index);
                best_shared = shared.len();
            }
        }
        if best.is_none() {
            best = chunk.lines.iter().position(|line| !line.trim().is_empty());
        }
        let line = chunk.first + best.unwrap_or(0);
        ranked.push(Ranked { chunk, score, line });
    }
    ranked.sort_by(|left, right| right.score.total_cmp(&left.score));
    ranked
}
