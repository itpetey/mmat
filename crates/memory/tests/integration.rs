use std::{
    collections::HashMap,
    sync::Arc,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use mmat_db::AsyncPgConnection;
use mmat_event_stream::{
    event::{EventId, EventType, EvidenceRef, RoleId, SemanticEvent},
    event_bus::EventBus,
    event_store::EventStore,
};
use mmat_memory::{
    artefact_store::ArtefactStore,
    attention::{AttentionConfig, AttentionEngine},
    error::{Error, Result},
    librarian::Librarian,
    provenance::ProvenanceEngine,
    store::MemoryStore,
    types::{Authority, Confidence, DecayPolicy, Memory, MemoryId, MemoryScope, MemoryType},
    vector_backend::VectorMemoryBackend,
};
use parking_lot::Mutex;
use qdrant_client::qdrant::Value;

type PgPool = mmat_db::Pool<AsyncPgConnection>;

#[derive(Default)]
struct FakeVectorBackend {
    results: Mutex<Vec<(MemoryId, f32)>>,
    upserts: Mutex<Vec<MemoryId>>,
    deleted: Mutex<Vec<MemoryId>>,
    fail_search: AtomicBool,
}

#[async_trait::async_trait]
impl VectorMemoryBackend for FakeVectorBackend {
    async fn upsert(
        &self,
        id: MemoryId,
        _embedding: Vec<f32>,
        _payload: HashMap<String, Value>,
    ) -> Result<()> {
        self.upserts.lock().push(id);
        Ok(())
    }

    async fn search(
        &self,
        _query_embedding: Vec<f32>,
        _limit: u64,
    ) -> Result<Vec<(MemoryId, f32)>> {
        if self.fail_search.load(Ordering::SeqCst) {
            return Err(Error::Qdrant("search failed".to_string()));
        }

        let configured = self.results.lock().clone();
        if !configured.is_empty() {
            return Ok(configured);
        }

        Ok(self.upserts.lock().iter().map(|id| (*id, 1.0)).collect())
    }

    async fn delete(&self, id: MemoryId) -> Result<()> {
        self.deleted.lock().push(id);
        self.upserts.lock().retain(|existing| *existing != id);
        Ok(())
    }
}

fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

async fn postgres_test_database(prefix: &str) -> Option<(PgPool, String, String)> {
    let base_url = std::env::var("MMAT_DB_URL").ok()?;
    let schema = format!("{}_{}", prefix, now_nanos());
    let admin_pool = mmat_db::new_pool(&base_url).await.ok()?;
    let mut conn = admin_pool.get().await.ok()?;
    mmat_db::execute_sql(&mut conn, &format!("CREATE SCHEMA \"{schema}\""))
        .await
        .ok()?;
    let separator = if base_url.contains('?') { '&' } else { '?' };
    let database_url = format!("{base_url}{separator}options=-c%20search_path%3D{schema}");
    let pool = mmat_db::new_pool(&database_url).await.ok()?;
    let migrate_pool = pool.clone();
    let mut migrator_conn = migrate_pool.get().await.ok()?;
    mmat_db::execute_sql(
        &mut migrator_conn,
        include_str!("../../db/migrations/2026-05-14-000001_init/up.sql"),
    )
    .await
    .ok()?;
    Some((pool, schema, database_url))
}

async fn drop_postgres_schema(pool: &PgPool, schema: &str) {
    if let Ok(mut conn) = pool.get().await {
        let _ = mmat_db::execute_sql(
            &mut conn,
            &format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"),
        )
        .await;
    }
}

async fn create_test_store(prefix: &str) -> Option<(Arc<MemoryStore>, PgPool, String)> {
    let (pool, schema, _url) = postgres_test_database(prefix).await?;
    let store = Arc::new(MemoryStore::new_with_pool(pool.clone()));
    Some((store, pool, schema))
}

#[tokio::test]
async fn integration_attention_proposal_carries_metadata() {
    let claim = SemanticEvent::new_claim_made(
        RoleId::new("llm"),
        "The API returns 404 for missing resources",
        vec![EvidenceRef {
            event_id: EventId::new(),
            description: "tool output".to_string(),
        }],
        0.85,
    );

    let evidence_refs = AttentionEngine::extract_evidence_refs(&claim);
    assert_eq!(evidence_refs.len(), 2);
    assert!(evidence_refs.iter().any(|r| r.event_id == claim.event_id()));

    let confidence = AttentionEngine::extract_confidence(&claim);
    assert!((confidence - 0.85).abs() < 0.01);

    let (memory_type, scope, authority) = AttentionEngine::infer_metadata(&claim);
    assert!(matches!(memory_type, MemoryType::Fact));
    assert!(matches!(scope, MemoryScope::Project));
    assert!(matches!(authority, Authority::LLMInference));
}

#[tokio::test]
async fn integration_attention_to_librarian_accepts_and_indexes_memory() {
    let (store, pool, schema) = match create_test_store("attention_accept").await {
        Some(v) => v,
        None => return,
    };
    let qdrant = Arc::new(FakeVectorBackend::default());
    let bus = Arc::new(EventBus::new(64));
    let attention = AttentionEngine::new(AttentionConfig {
        salience_batch_size: 1,
        ..AttentionConfig::default()
    });
    let librarian = Librarian::new(store.clone(), qdrant.clone(), Duration::from_secs(3600));
    let mut accepted_rx = bus.subscribe(&[EventType::MemoryAccepted]);

    let attention_handle = tokio::spawn({
        let bus = bus.clone();
        let store = store.clone();
        let qdrant = qdrant.clone();
        async move { attention.run(bus, store, qdrant).await }
    });
    let librarian_handle = tokio::spawn({
        let bus = bus.clone();
        async move { librarian.run(bus).await }
    });

    tokio::time::sleep(Duration::from_millis(25)).await;

    let claim = SemanticEvent::new_claim_made(
        RoleId::new("llm"),
        "The database schema requires a migration because the new column was observed",
        vec![EvidenceRef {
            event_id: EventId::new(),
            description: "tool output".to_string(),
        }],
        0.85,
    );
    bus.publish(claim).unwrap();

    let accepted = tokio::time::timeout(Duration::from_secs(2), accepted_rx.recv())
        .await
        .expect("timeout waiting for MemoryAccepted")
        .expect("channel closed");
    let SemanticEvent::MemoryAccepted { memory_id, .. } = accepted.as_ref() else {
        panic!("unexpected event")
    };

    let memory_id = MemoryId(memory_id.0);
    assert!(store.get_by_id(memory_id).unwrap().is_some());
    let similar = store
        .search_similar(vec![0.0; 64], 10, qdrant.as_ref())
        .await
        .unwrap();
    assert_eq!(similar.len(), 1);
    assert_eq!(similar[0].0, memory_id);

    attention_handle.abort();
    librarian_handle.abort();
    drop_postgres_schema(&pool, &schema).await;
}

#[tokio::test]
async fn integration_contradiction_higher_authority() {
    let (store, pool, schema) = match create_test_store("contradiction").await {
        Some(v) => v,
        None => return,
    };
    let qdrant = Arc::new(FakeVectorBackend::default());
    let bus = Arc::new(EventBus::new(64));

    let old_memory = Memory::builder()
        .memory_type(MemoryType::Fact)
        .content("The API endpoint returns status 200 as of the latest test")
        .scope(MemoryScope::Project)
        .authority(Authority::LLMInference)
        .confidence(Confidence::new(0.7).unwrap())
        .source_agent(RoleId::new("llm"))
        .build()
        .unwrap();

    store.insert(&old_memory).unwrap();

    let librarian = Librarian::new(store.clone(), qdrant.clone(), Duration::from_secs(3600));
    let mut accepted_rx = bus.subscribe(&[EventType::MemoryAccepted]);
    let handle = tokio::spawn({
        let bus = bus.clone();
        async move { librarian.run(bus).await }
    });

    tokio::time::sleep(Duration::from_millis(25)).await;
    bus.publish(SemanticEvent::new_memory_proposed(
        RoleId::new("compiler"),
        "Fact",
        "The API endpoint returns status 500 as of the latest test",
        "Project",
        RoleId::new("compiler"),
        vec![],
        0.95,
    ))
    .unwrap();

    tokio::time::timeout(Duration::from_secs(2), accepted_rx.recv())
        .await
        .expect("timeout waiting for MemoryAccepted")
        .expect("channel closed");

    let old = store.get_by_id(old_memory.id).unwrap().unwrap();
    assert!(old.superseded_by.is_some());
    assert_eq!(qdrant.deleted.lock().as_slice(), &[old_memory.id]);

    let chain = store.get_supersession_chain(old_memory.id).unwrap();
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0].id, old_memory.id);
    assert_eq!(chain[1].supersedes, Some(old_memory.id));
    handle.abort();
    drop_postgres_schema(&pool, &schema).await;
}

#[tokio::test]
async fn integration_decay_scan_supersedes_stale() {
    let Some((store, pool, schema)) = create_test_store("decay").await else {
        return;
    };
    let qdrant = Arc::new(FakeVectorBackend::default());
    let bus = Arc::new(EventBus::new(64));

    let stale_memory = Memory::builder()
        .memory_type(MemoryType::Fact)
        .content("This is a stale fact")
        .scope(MemoryScope::Ephemeral)
        .authority(Authority::LLMInference)
        .confidence(Confidence::new(0.5).unwrap())
        .decay_policy(DecayPolicy::StaleAfterDays(0))
        .source_agent(RoleId::new("llm"))
        .build()
        .unwrap();

    store.insert(&stale_memory).unwrap();
    let librarian = Librarian::new(store.clone(), qdrant.clone(), Duration::from_millis(25));
    let mut superseded_rx = bus.subscribe(&[EventType::MemorySuperseded]);
    let handle = tokio::spawn({
        let bus = bus.clone();
        async move { librarian.run(bus).await }
    });

    let superseded = tokio::time::timeout(Duration::from_secs(2), superseded_rx.recv())
        .await
        .expect("timeout waiting for MemorySuperseded")
        .expect("channel closed");
    assert_eq!(superseded.variant_name(), "MemorySuperseded");

    let decayed = store.query_decayed().unwrap();
    assert!(decayed.is_empty());
    assert_eq!(qdrant.deleted.lock().as_slice(), &[stale_memory.id]);
    handle.abort();
    drop_postgres_schema(&pool, &schema).await;
}

#[tokio::test]
async fn integration_grounding_gate_rejects_ungrounded_llm() {
    let Some((store, pool, schema)) = create_test_store("grounding").await else {
        return;
    };
    let qdrant = Arc::new(FakeVectorBackend::default());
    let bus = Arc::new(EventBus::new(64));
    let librarian = Librarian::new(store.clone(), qdrant, Duration::from_secs(3600));
    let mut rejected_rx = bus.subscribe(&[EventType::MemoryRejected]);
    let handle = tokio::spawn({
        let bus = bus.clone();
        async move { librarian.run(bus).await }
    });

    tokio::time::sleep(Duration::from_millis(25)).await;
    bus.publish(SemanticEvent::new_memory_proposed(
        RoleId::new("llm"),
        "Decision",
        "We should change the API because the current behaviour is unclear",
        "Project",
        RoleId::new("llm"),
        vec![],
        0.5,
    ))
    .unwrap();

    let rejected = tokio::time::timeout(Duration::from_secs(2), rejected_rx.recv())
        .await
        .expect("timeout waiting for MemoryRejected")
        .expect("channel closed");
    assert!(matches!(
        rejected.as_ref(),
        SemanticEvent::MemoryRejected { rejection_gate, .. } if rejection_gate == "grounding"
    ));
    assert!(
        store
            .query_by_type(MemoryType::Decision)
            .unwrap()
            .is_empty()
    );
    handle.abort();
    drop_postgres_schema(&pool, &schema).await;
}

#[tokio::test]
async fn integration_memory_lifecycle() {
    let Some((store, pool, schema)) = create_test_store("lifecycle").await else {
        return;
    };
    let memory = Memory::builder()
        .memory_type(MemoryType::Fact)
        .content("This is a durable test fact with sufficient length")
        .scope(MemoryScope::Project)
        .authority(Authority::UserInstruction)
        .confidence(Confidence::new(0.9).unwrap())
        .source_agent(RoleId::new("user"))
        .build()
        .unwrap();

    store.insert(&memory).unwrap();

    let retrieved = store.get_by_id(memory.id).unwrap().unwrap();
    assert_eq!(retrieved.content, memory.content);
    assert_eq!(retrieved.memory_type, MemoryType::Fact);
    assert_eq!(retrieved.scope, MemoryScope::Project);
    drop_postgres_schema(&pool, &schema).await;
}

#[tokio::test]
async fn integration_near_duplicate_suppression() {
    let Some((store, pool, schema)) = create_test_store("near_dup").await else {
        return;
    };
    let qdrant = Arc::new(FakeVectorBackend::default());

    let memory = Memory::builder()
        .memory_type(MemoryType::Fact)
        .content("This is a durable test fact with sufficient length")
        .scope(MemoryScope::Project)
        .authority(Authority::UserInstruction)
        .confidence(Confidence::new(0.9).unwrap())
        .source_agent(RoleId::new("user"))
        .build()
        .unwrap();
    store.insert(&memory).unwrap();
    qdrant.results.lock().push((memory.id, 0.99));
    let before = store
        .get_by_id(memory.id)
        .unwrap()
        .unwrap()
        .last_accessed_at;

    let bus = Arc::new(EventBus::new(64));
    let mut proposed_rx = bus.subscribe(&[EventType::MemoryProposed]);
    let attention = AttentionEngine::new(AttentionConfig {
        salience_batch_size: 1,
        ..AttentionConfig::default()
    });
    let handle = tokio::spawn({
        let bus = bus.clone();
        let store = store.clone();
        let qdrant = qdrant.clone();
        async move { attention.run(bus, store, qdrant).await }
    });

    tokio::time::sleep(Duration::from_millis(25)).await;
    bus.publish(SemanticEvent::new_claim_made(
        RoleId::new("llm"),
        "This is a durable test fact with sufficient length",
        vec![EvidenceRef {
            event_id: EventId::new(),
            description: "tool output".to_string(),
        }],
        0.9,
    ))
    .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;
    let after = store
        .get_by_id(memory.id)
        .unwrap()
        .unwrap()
        .last_accessed_at;
    assert!(after > before);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), proposed_rx.recv())
            .await
            .is_err()
    );
    handle.abort();
    drop_postgres_schema(&pool, &schema).await;
}

#[tokio::test]
async fn integration_provenance_trace() {
    let Some((store, pool, schema)) = create_test_store("provenance").await else {
        return;
    };
    let event_store = Arc::new(EventStore::empty());

    let engine = ProvenanceEngine::new();

    let tool_event = SemanticEvent::new_tool_executed(
        RoleId::new("worker"),
        "cargo_test",
        "{}",
        0,
        "All tests passed",
        "",
        0,
    );
    let tool_event_id = tool_event.event_id();
    event_store.insert(&tool_event).unwrap();

    let evidence_ref = EvidenceRef {
        event_id: tool_event_id,
        description: "test output".to_string(),
    };

    let claim = SemanticEvent::new_claim_made(
        RoleId::new("llm"),
        "All tests are passing",
        vec![evidence_ref],
        0.9,
    );
    event_store.insert(&claim).unwrap();

    let memory = Memory::builder()
        .memory_type(MemoryType::Fact)
        .content("All tests are passing")
        .scope(MemoryScope::Project)
        .authority(Authority::LLMInference)
        .confidence(Confidence::new(0.9).unwrap())
        .evidence_refs(vec![claim.event_id()])
        .source_agent(RoleId::new("llm"))
        .build()
        .unwrap();

    store.insert(&memory).unwrap();

    let retrieved = store.get_by_id(memory.id).unwrap().unwrap();
    assert_eq!(retrieved.evidence_refs.len(), 1);

    let trace = engine.trace_memory(&retrieved, &event_store).unwrap();
    assert!(!trace.is_empty());
    drop_postgres_schema(&pool, &schema).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_artefact_store_round_trip_and_transactional_event() {
    let Some((pool, schema, database_url)) = postgres_test_database("artefact_store").await else {
        return;
    };

    let store = ArtefactStore::new_postgres(&database_url).unwrap();
    let bus = EventBus::new(16);
    let mut artefact_rx = bus.subscribe(&[EventType::ArtefactProduced]);
    let payload = r#"{"summary":"ok"}"#;

    let stored = store.store("audit_report", payload).await.unwrap();
    assert!(stored.storage_uri.starts_with("db://artefacts/"));
    assert_eq!(
        store.get_payload(&stored.storage_uri).await.unwrap(),
        Some(payload.to_string())
    );

    let event_ref = store
        .store_and_publish_event("audit_report", payload, "auditor", "auditor", &bus)
        .await
        .unwrap();
    assert!(event_ref.storage_uri.starts_with("db://artefacts/"));
    assert_eq!(
        artefact_rx.recv().await.unwrap().variant_name(),
        "ArtefactProduced"
    );

    drop_postgres_schema(&pool, &schema).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_memory_store_crud_queries_and_supersession() {
    let Some((pool, schema, database_url)) = postgres_test_database("memory_store").await else {
        return;
    };

    let store = MemoryStore::new(&database_url).unwrap();

    let old = Memory::builder()
        .memory_type(MemoryType::Fact)
        .content("The API returns 200 for health checks")
        .scope(MemoryScope::Project)
        .authority(Authority::UserInstruction)
        .confidence(Confidence::new(0.9).unwrap())
        .decay_policy(DecayPolicy::StaleAfterDays(0))
        .source_agent(RoleId::new("user"))
        .build()
        .unwrap();
    let new = Memory::builder()
        .memory_type(MemoryType::Fact)
        .content("The API returns 204 for health checks")
        .scope(MemoryScope::Project)
        .authority(Authority::AcceptedADR)
        .confidence(Confidence::new(0.95).unwrap())
        .supersedes(old.id)
        .source_agent(RoleId::new("architect"))
        .build()
        .unwrap();

    store.insert(&old).unwrap();
    store.insert(&new).unwrap();
    store.supersede(old.id, new.id).unwrap();

    assert_eq!(
        store.get_by_id(old.id).unwrap().unwrap().superseded_by,
        Some(new.id)
    );
    assert_eq!(store.query_by_type(MemoryType::Fact).unwrap().len(), 1);
    assert_eq!(store.query_by_scope(MemoryScope::Project).unwrap().len(), 1);
    assert_eq!(
        store
            .query_by_authority(Authority::CompilerOutput, Authority::SpeculativeReasoning)
            .unwrap()
            .len(),
        1
    );
    assert!(store.query_decayed().unwrap().is_empty());

    let chain = store.get_supersession_chain(old.id).unwrap();
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0].id, old.id);
    assert_eq!(chain[1].id, new.id);

    drop_postgres_schema(&pool, &schema).await;
}
