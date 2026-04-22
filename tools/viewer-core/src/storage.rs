#[derive(Debug, Clone, Default)]
pub struct ChunkStorageConfig {
    pub chunk_size: u32,
}

#[derive(Debug, Clone, Default)]
pub struct ChunkStorageStats {
    pub chunk_count: usize,
    pub non_empty_chunk_count: usize,
}
