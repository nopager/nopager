use nopager_db::{Database, IncidentTrigger, ProtectedAppSetup};
use serde_json::json;

#[tokio::test]
async fn setup_and_incident_queries_round_trip() {
    let Ok(url) = std::env::var("NOPAGER_TEST_DATABASE_URL") else {
        eprintln!("NOPAGER_TEST_DATABASE_URL is not set; skipping PostgreSQL integration test");
        return;
    };
    let database = Database::connect(&url).await.unwrap();
    database.migrate().await.unwrap();
    assert!(!database.admin_exists().await.unwrap());
    database
        .create_local_admin("admin", "test-password-hash")
        .await
        .unwrap();

    let project_id = database
        .create_protected_app(&ProtectedAppSetup {
            name: "Test Production".into(),
            slug: "test-production".into(),
            repo_owner: "nopager".into(),
            repo_name: "fixture".into(),
            production_url: "https://example.com/".into(),
            health_check_url: "https://example.com/health".into(),
            safety_mode: "safe".into(),
            github_external_account_id: "installation-1".into(),
            github_external_project_id: "nopager/fixture".into(),
            github_credentials: "encrypted-github".into(),
            github_metadata: json!({ "repoId": 1, "baseBranch": "main" }),
            vercel_external_account_id: "team-1".into(),
            vercel_external_project_id: "project-1".into(),
            vercel_credentials: "encrypted-vercel".into(),
            vercel_metadata: json!({ "teamId": "team-1", "projectName": "fixture" }),
            provider_external_account_id: "openai".into(),
            provider_credentials: "encrypted-provider".into(),
            provider_metadata: json!({ "provider": "openai", "model": "test", "keySuffix": "1234" }),
            initial_deployment_id: "deployment-1".into(),
            initial_deployment_url: "https://fixture.vercel.app".into(),
            initial_deployment_sha: "0123456789abcdef".into(),
        })
        .await
        .unwrap();
    let settings = database.app_settings().await.unwrap().unwrap();
    let project_id_text = project_id.to_string();
    assert_eq!(
        settings
            .pointer("/project/id")
            .and_then(|value| value.as_str()),
        Some(project_id_text.as_str())
    );
    assert_eq!(
        database
            .overview()
            .await
            .unwrap()
            .get("configured")
            .and_then(|value| value.as_bool()),
        Some(true)
    );

    let incident_id = database
        .open_incident(&IncidentTrigger {
            project_id,
            deduplication_key: "integration-test".into(),
            trigger_type: "HEALTH_CHECK".into(),
            severity: "high".into(),
            title: "Integration health failed".into(),
            metadata: json!({ "statusCode": 503 }),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        database
            .incidents(10)
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let detail = database
        .incident_detail(incident_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        detail.get("title").and_then(|value| value.as_str()),
        Some("Integration health failed")
    );
    assert_eq!(
        database
            .set_safety_mode("autopilot_experimental", "test")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        database.set_protection_paused(true, "test").await.unwrap(),
        1
    );
}
