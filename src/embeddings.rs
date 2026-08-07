use anyhow::{Context, Result};
use fastembed::{TextEmbedding, InitOptions, EmbeddingModel};

pub fn build_embedder() -> Result<TextEmbedding> {
    TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::NomicEmbedTextV15Q)
            .with_show_download_progress(true),
    )
    .context("failed to initialize Nomic Embed Text v1.5 (quantized)")
}

pub fn get_embedding(model: &mut TextEmbedding, text: &str) -> Result<Vec<f32>> {
    let embeddings = model
        .embed(vec![text.to_string()], None)
        .context("embedding request failed")?;

    embeddings.into_iter().next().context("no embedding vector returned")
}

pub fn get_embeddings_batch(model: &mut TextEmbedding, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
    model.embed(texts, None).context("batch embedding request failed")
}