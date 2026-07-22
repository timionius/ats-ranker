use anyhow::{Context, Result};
use fastembed::{TextEmbedding, InitOptions, EmbeddingModel};

pub fn build_embedder() -> Result<TextEmbedding> {
    TextEmbedding::try_new(InitOptions::new(EmbeddingModel::BGESmallENV15))
        .context("failed to initialize BGE-small-en-v1.5 embedding model")
}

pub fn get_embedding(model: &mut TextEmbedding, text: &str) -> Result<Vec<f32>> {
    let embeddings = model
        .embed(vec![text.to_string()], None)
        .context("embedding request failed")?;

    embeddings
        .into_iter()
        .next()
        .context("no embedding vector returned")
}

pub fn get_embeddings_batch(model: &mut TextEmbedding, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
    model
        .embed(texts, None)
        .context("batch embedding request failed")
}