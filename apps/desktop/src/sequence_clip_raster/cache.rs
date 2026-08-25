use super::*;

pub(super) struct ActiveRasterJob {
    pub(super) request_id: u32,
    pub(super) document_key: GuiDocumentRequest,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct RasterCacheKey {
    pub(super) path: String,
    pub(super) object_key: Option<String>,
    pub(super) effect_id: u32,
    pub(super) display_column_count: u32,
    pub(super) display_row_count: u32,
    pub(super) settings: EffectRasterSettings,
}

impl Eq for RasterCacheKey {}

impl RasterCacheKey {
    pub(super) fn new(
        document: &GuiDocumentRequest,
        display_row_count: u32,
        settings: &EffectRasterSettings,
        effect_id: u32,
        display_column_count: u32,
    ) -> Self {
        Self {
            path: document.path.clone(),
            object_key: document.object_key.clone(),
            effect_id,
            display_column_count,
            display_row_count,
            settings: settings.clone(),
        }
    }

    pub(super) fn matches_request(
        &self,
        document: &GuiDocumentRequest,
        display_row_count: u32,
        settings: &EffectRasterSettings,
    ) -> bool {
        self.path == document.path
            && self.object_key == document.object_key
            && self.display_row_count == display_row_count
            && self.settings == *settings
    }
}

impl Hash for RasterCacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.path.hash(state);
        self.object_key.hash(state);
        self.effect_id.hash(state);
        self.display_column_count.hash(state);
        self.display_row_count.hash(state);
        hash_effect_raster_settings(&self.settings, state);
    }
}

#[derive(Clone)]
pub(super) struct CachedRasterResult {
    pub(super) signature: RenderInputSignature,
    pub(super) byte_len: usize,
    pub(super) last_used: u64,
    pub(super) result: CachedRasterValue,
}

#[derive(Clone)]
pub(super) enum CachedRasterValue {
    Raster(CachedRasterPayload),
    Error(SequenceClipRasterError),
}

#[derive(Clone)]
pub(super) struct CachedRasterPayload {
    pub(super) raster: SequenceClipRaster,
    pub(super) pixels_rgba: Arc<Vec<u8>>,
    pub(super) token: String,
}

impl CachedRasterPayload {
    pub(super) fn byte_len(&self) -> usize {
        self.pixels_rgba.len()
    }
}
