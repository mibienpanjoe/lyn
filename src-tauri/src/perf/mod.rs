//! Reproducible latency budgets for T33 (INV performance targets).

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        sync::Mutex,
        time::{Duration, Instant},
    };

    use serde_json::json;
    use tempfile::tempdir;

    use crate::{
        capture::session::CaptureSessionService,
        commands::capture::save_text_capture_value,
        contract::{ContextCandidate, ContextProviderKind, ContextResolution, SearchCapturesInput},
        library::search::SearchService,
        media::staging::MediaStore,
        storage::{Database, contexts::ContextRepository},
    };

    const TEXT_SAVE_BUDGET: Duration = Duration::from_millis(150);
    const SEARCH_10K_BUDGET: Duration = Duration::from_millis(200);
    const SAMPLES: usize = 40;

    fn percentile(mut samples: Vec<Duration>, p: f64) -> Duration {
        samples.sort_unstable();
        let index = ((samples.len() as f64 - 1.0) * p).round() as usize;
        samples[index]
    }

    fn warm_text_save_fixture() -> (Mutex<Database>, Mutex<CaptureSessionService>) {
        let database = Database::open_in_memory().unwrap();
        let context = ContextRepository::new(database.connection())
            .create_standalone("Perf")
            .unwrap();
        let service = Mutex::new(CaptureSessionService::default());
        let session = service.lock().unwrap().get_or_prepare();
        service
            .lock()
            .unwrap()
            .set_context_resolution(
                session.session_id,
                ContextResolution::Resolved {
                    candidate: ContextCandidate {
                        context,
                        branch_name: None,
                        provider: ContextProviderKind::Manual,
                        requires_confirmation: false,
                    },
                    selection: None,
                },
            )
            .unwrap();
        (Mutex::new(database), service)
    }

    #[test]
    fn text_save_p95_stays_within_the_provisional_budget() {
        let (database, service) = warm_text_save_fixture();
        let session_id = service.lock().unwrap().active_session().unwrap().session_id;

        // Warm the path once so first-run SQLite page cache is excluded.
        let warm = save_text_capture_value(
            json!({ "sessionId": session_id, "textBody": "warm" }),
            &database,
            &service,
        );
        assert!(matches!(warm, crate::error::CommandResult::Success { .. }));

        let mut samples = Vec::with_capacity(SAMPLES);
        for index in 0..SAMPLES {
            // Each save needs a fresh session because session_id is unique.
            let next = service.lock().unwrap().get_or_prepare();
            let context = {
                let database = database.lock().unwrap();
                ContextRepository::new(database.connection())
                    .create_standalone(&format!("Perf {index}"))
                    .unwrap()
            };
            service
                .lock()
                .unwrap()
                .set_context_resolution(
                    next.session_id,
                    ContextResolution::Resolved {
                        candidate: ContextCandidate {
                            context,
                            branch_name: None,
                            provider: ContextProviderKind::Manual,
                            requires_confirmation: false,
                        },
                        selection: None,
                    },
                )
                .unwrap();

            let started = Instant::now();
            let result = save_text_capture_value(
                json!({
                    "sessionId": next.session_id,
                    "textBody": format!("perf body {index}")
                }),
                &database,
                &service,
            );
            samples.push(started.elapsed());
            assert!(matches!(
                result,
                crate::error::CommandResult::Success { .. }
            ));
        }

        let p50 = percentile(samples.clone(), 0.50);
        let p95 = percentile(samples.clone(), 0.95);
        eprintln!(
            "perf text-save samples={SAMPLES} p50={:.3}ms p95={:.3}ms budget={}ms",
            p50.as_secs_f64() * 1000.0,
            p95.as_secs_f64() * 1000.0,
            TEXT_SAVE_BUDGET.as_millis()
        );
        assert!(
            p95 <= TEXT_SAVE_BUDGET,
            "text-save p95 {}ms exceeded {}ms budget",
            p95.as_millis(),
            TEXT_SAVE_BUDGET.as_millis()
        );
    }

    #[test]
    fn search_over_ten_thousand_captures_p95_stays_within_budget() {
        let database = Database::open_in_memory().unwrap();
        database
            .connection()
            .execute_batch(
                "INSERT INTO contexts (id, kind, name, created_at, updated_at)
                 VALUES ('11111111-1111-4111-8111-111111111111', 'project', 'Lyn',
                         '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z');",
            )
            .unwrap();
        database
            .connection()
            .execute_batch("BEGIN IMMEDIATE")
            .unwrap();
        {
            let mut insert = database
                .connection()
                .prepare(
                    "INSERT INTO captures (id, session_id, context_id, kind, text_body, caption,
                 caption_source, branch_name, source_app, source_window_title, captured_at, updated_at)
                 VALUES (?1, ?2, '11111111-1111-4111-8111-111111111111', 'text',
                 'bounded needle', NULL, NULL, 'main', NULL, NULL,
                 '2026-08-01T00:00:00Z', '2026-08-01T00:00:00Z')",
                )
                .unwrap();
            for value in 0..10_000_u32 {
                let id = format!("00000000-0000-4000-8000-{value:012}");
                let session = format!("10000000-0000-4000-8000-{value:012}");
                insert.execute((id, session)).unwrap();
            }
        }
        database.connection().execute_batch("COMMIT").unwrap();
        // FTS projection is trigger-maintained; rebuild once so the index is warm.
        let media = MediaStore::open(tempdir().unwrap().path()).unwrap();
        let service = SearchService::new(database.connection(), &media);
        service.rebuild().unwrap();

        let request = SearchCapturesInput {
            query: "needle".to_owned(),
            context_id: None,
            branch_name: None,
            capture_kinds: vec![],
            captured_from: None,
            captured_to: None,
            cursor: None,
            limit: 25,
        };
        let warm = service.search(&request).unwrap();
        assert_eq!(warm.items.len(), 25);

        let mut samples = Vec::with_capacity(SAMPLES);
        for _ in 0..SAMPLES {
            let started = Instant::now();
            let page = service.search(&request).unwrap();
            samples.push(started.elapsed());
            assert_eq!(page.items.len(), 25);
        }

        let p50 = percentile(samples.clone(), 0.50);
        let p95 = percentile(samples, 0.95);
        eprintln!(
            "perf search-10k samples={SAMPLES} p50={:.3}ms p95={:.3}ms budget={}ms",
            p50.as_secs_f64() * 1000.0,
            p95.as_secs_f64() * 1000.0,
            SEARCH_10K_BUDGET.as_millis()
        );
        assert!(
            p95 <= SEARCH_10K_BUDGET,
            "search-10k p95 {}ms exceeded {}ms budget",
            p95.as_millis(),
            SEARCH_10K_BUDGET.as_millis()
        );
    }

    #[test]
    fn startup_reconciliation_of_orphaned_staging_stays_bounded() {
        let directory = tempdir().unwrap();
        let mut media = MediaStore::open(directory.path()).unwrap();
        let session = crate::contract::CaptureSessionId::new();
        for _ in 0..32 {
            media
                .stage_audio_wav(session, b"wav bytes for orphan", 40)
                .unwrap();
        }
        let started = Instant::now();
        media.reconcile(&HashSet::new()).unwrap();
        let elapsed = started.elapsed();
        eprintln!("perf reconcile-orphans elapsed={}ms", elapsed.as_millis());
        assert!(
            elapsed < Duration::from_millis(500),
            "startup reconciliation took {}ms",
            elapsed.as_millis()
        );
    }
}
