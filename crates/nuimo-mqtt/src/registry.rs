//! In-memory glyph registry hydrated from `system/glyphs/{name}` retained
//! publishes by weave-server. Empty payload = tombstone.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use weave_contracts::Glyph;

#[derive(Default, Clone)]
pub struct GlyphRegistry {
    inner: Arc<RwLock<HashMap<String, Glyph>>>,
}

impl GlyphRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a retained message payload. Name is taken from the last segment
    /// of the topic. Empty payload removes the entry.
    pub async fn apply(&self, name: &str, payload: &[u8]) {
        if payload.is_empty() {
            let mut g = self.inner.write().await;
            g.remove(name);
            tracing::debug!(%name, "glyph tombstone applied");
            return;
        }
        match serde_json::from_slice::<Glyph>(payload) {
            Ok(glyph) => {
                let mut g = self.inner.write().await;
                g.insert(glyph.name.clone(), glyph);
                tracing::debug!(%name, "glyph upserted");
            }
            Err(e) => {
                tracing::warn!(%name, error = %e, "failed to parse glyph payload");
            }
        }
    }

    pub async fn get(&self, name: &str) -> Option<Glyph> {
        self.inner.read().await.get(name).cloned()
    }
}

/// Parse `system/glyphs/{name}` into its final segment. Returns None for
/// any topic that doesn't match.
pub fn topic_to_name(topic: &str) -> Option<&str> {
    topic.strip_prefix("system/glyphs/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn apply_inserts_and_tombstones() {
        let r = GlyphRegistry::new();
        let payload = br#"{"name":"play","pattern":"**","builtin":false}"#;
        r.apply("play", payload).await;
        let g = r.get("play").await.unwrap();
        assert_eq!(g.pattern, "**");
        assert!(!g.builtin);

        r.apply("play", &[]).await;
        assert!(r.get("play").await.is_none());
    }

    #[test]
    fn topic_to_name_matches_prefix() {
        assert_eq!(topic_to_name("system/glyphs/play"), Some("play"));
        assert_eq!(
            topic_to_name("system/glyphs/volume_bar"),
            Some("volume_bar")
        );
        assert_eq!(topic_to_name("device/nuimo/x/feedback/y"), None);
    }
}
