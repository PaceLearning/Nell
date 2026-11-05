//! The local sentence model that gives a text its vector, run through the ONNX runtime inside nell.

use std::fmt;
use std::path::Path;

use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

/// The model nell uses: all-MiniLM-L6-v2, 384 numbers per vector, about 90 MB on disk. It is the smallest model
/// fastembed offers, 22 million weights, and was trained on English and on questions and answers about code.
/// Its 8 bit variant is not used: fastembed cannot batch a dynamically quantized model, and a vector from one
/// would depend on what else was in its batch.
pub const MODEL: EmbeddingModel = EmbeddingModel::AllMiniLML6V2;

/// How many tokens of a text the model reads. The rest of a long chunk is left out.
const MAX_TOKENS: usize = 256;

/// How many texts go through the model at once.
const BATCH: usize = 32;

/// Why the model could not be loaded or could not embed.
pub enum ModelError {
    /// The model files could not be downloaded or read, or the ONNX runtime could not start.
    Load(fastembed::Error),
    /// The model could not turn the texts into vectors.
    Embed(fastembed::Error),
    /// The model gave back a different number of vectors than texts.
    Count {
        /// How many texts were given.
        asked: usize,
        /// How many vectors came back.
        given: usize,
    },
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (what, error) = match self {
            ModelError::Load(error) => ("the model could not be loaded", error),
            ModelError::Embed(error) => ("the model could not embed", error),
            ModelError::Count { asked, given } => {
                return write!(f, "the model gave {given} vectors for {asked} texts");
            }
        };
        write!(f, "{what}: {error}")?;
        let mut source = std::error::Error::source(error);
        while let Some(inner) = source {
            write!(f, ": {inner}")?;
            source = inner.source();
        }
        Ok(())
    }
}

/// The loaded model.
pub struct Model {
    inner: TextEmbedding,
}

impl Model {
    /// Loads the model, with its files in `folder`. The first load downloads them, about 90 MB, into `folder`.
    /// After that nothing needs the network.
    pub fn load(folder: &Path) -> Result<Model, ModelError> {
        let options = TextInitOptions::new(MODEL)
            .with_cache_dir(folder.to_path_buf())
            .with_show_download_progress(false)
            .with_max_length(MAX_TOKENS);
        let inner = TextEmbedding::try_new(options).map_err(ModelError::Load)?;
        Ok(Model { inner })
    }

    /// One vector per text, in the same order.
    pub fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>, ModelError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let vectors = self.inner.embed(texts, Some(BATCH)).map_err(ModelError::Embed)?;
        if vectors.len() != texts.len() {
            return Err(ModelError::Count { asked: texts.len(), given: vectors.len() });
        }
        Ok(vectors)
    }
}
