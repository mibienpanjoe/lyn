//! Optional local intelligence adapters.

pub(crate) struct UnavailableLocalIntelligence;

impl UnavailableLocalIntelligence {
    pub(crate) fn available(&self) -> bool {
        false
    }
}

impl crate::enrichment::EnrichmentProcessor for UnavailableLocalIntelligence {
    fn generate(
        &mut self,
        _job: &crate::enrichment::EnrichmentJob,
    ) -> Result<Option<String>, &'static str> {
        Err("MODEL_NOT_AVAILABLE")
    }
}
