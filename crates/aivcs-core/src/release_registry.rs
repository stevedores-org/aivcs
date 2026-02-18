use oxidized_state::{
    ContentDigest, ReleaseMetadata, ReleaseRecord, ReleaseRegistry, StorageResult,
};

/// Thin API layer over a release registry backend.
pub struct ReleaseRegistryApi<R> {
    registry: R,
}

impl<R> ReleaseRegistryApi<R>
where
    R: ReleaseRegistry,
{
    pub fn new(registry: R) -> Self {
        Self { registry }
    }

    pub async fn promote(
        &self,
        name: &str,
        spec_digest: &ContentDigest,
        promoted_by: &str,
        version_label: Option<String>,
        notes: Option<String>,
    ) -> StorageResult<ReleaseRecord> {
        let metadata = ReleaseMetadata {
            version_label,
            promoted_by: promoted_by.to_string(),
            notes,
        };
        self.registry.promote(name, spec_digest, metadata).await
    }

    pub async fn rollback(&self, name: &str) -> StorageResult<ReleaseRecord> {
        self.registry.rollback(name).await
    }

    pub async fn current(&self, name: &str) -> StorageResult<Option<ReleaseRecord>> {
        self.registry.current(name).await
    }

    pub async fn history(&self, name: &str) -> StorageResult<Vec<ReleaseRecord>> {
        self.registry.history(name).await
    }
}

#[cfg(test)]
mod tests {
    use super::ReleaseRegistryApi;
    use oxidized_state::fakes::MemoryReleaseRegistry;
    use oxidized_state::ContentDigest;

    #[tokio::test]
    async fn promote_promote_rollback_keeps_append_only_history() {
        let api = ReleaseRegistryApi::new(MemoryReleaseRegistry::new());
        let name = "agent-registry";
        let d1 = ContentDigest::from_bytes(b"spec-v1");
        let d2 = ContentDigest::from_bytes(b"spec-v2");

        let first = api
            .promote(
                name,
                &d1,
                "ci",
                Some("v1.0.0".to_string()),
                Some("first release".to_string()),
            )
            .await
            .expect("first promote");
        assert_eq!(first.spec_digest, d1);

        let second = api
            .promote(
                name,
                &d2,
                "ci",
                Some("v1.1.0".to_string()),
                Some("second release".to_string()),
            )
            .await
            .expect("second promote");
        assert_eq!(second.spec_digest, d2);

        let rolled_back = api.rollback(name).await.expect("rollback");
        assert_eq!(rolled_back.spec_digest, d1);

        let current = api
            .current(name)
            .await
            .expect("current")
            .expect("current exists");
        assert_eq!(current.spec_digest, d1);

        let history = api.history(name).await.expect("history");
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].spec_digest, d1);
        assert_eq!(history[1].spec_digest, d2);
        assert_eq!(history[2].spec_digest, d1);
    }
}
