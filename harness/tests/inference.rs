use std::io::Cursor;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use mutagen_harness::adapter::{HostKind, resolved_host_profile};
use mutagen_harness::config::WorkflowConfig;
use mutagen_harness::inference::{ChatCompletionOptions, ChatMessage, complete_chat};
use mutagen_harness::model_registry::{
    InferenceProvider, REGISTRY, find_model, list_models_for, resolve_provider_model_id,
};

#[test]
fn ollama_and_lmstudio_profiles_are_advisory_and_serial() {
    let workflow_config = WorkflowConfig {
        max_parallel_slices: 4,
        ..WorkflowConfig::default()
    };

    for host in [HostKind::Ollama, HostKind::LmStudio] {
        let profile = resolved_host_profile(host, &workflow_config);
        assert_eq!(profile.host, host);

        // OpenAI-compatible servers don't give us harness-grade controls.
        assert_eq!(
            profile.scope_enforcement,
            mutagen_harness::adapter::ScopeEnforcementMode::Advisory,
            "{host:?}"
        );
        assert_eq!(
            profile.parallel_dispatch,
            mutagen_harness::adapter::ParallelDispatchMode::SerialOnly,
            "{host:?}"
        );
        assert_eq!(profile.effective_max_parallel_slices, 1);
        assert!(
            profile
                .degraded_features
                .iter()
                .any(|feature| feature == "pre_write_scope_enforcement"),
            "expected scope enforcement to be marked degraded for {host:?}"
        );
    }
}

#[test]
fn registry_advertises_at_least_one_id_per_provider() {
    let ollama = list_models_for(InferenceProvider::Ollama);
    let lmstudio = list_models_for(InferenceProvider::LmStudio);

    assert!(
        !ollama.is_empty(),
        "expected at least one Ollama-supported model"
    );
    assert!(
        !lmstudio.is_empty(),
        "expected at least one LM Studio-supported model"
    );
    assert_eq!(
        REGISTRY.len(),
        7,
        "registry size shifted — confirm intentional",
    );
}

#[test]
fn from_host_kind_resolves_inference_providers() {
    assert_eq!(
        InferenceProvider::from_host_kind(HostKind::Ollama),
        Some(InferenceProvider::Ollama)
    );
    assert_eq!(
        InferenceProvider::from_host_kind(HostKind::LmStudio),
        Some(InferenceProvider::LmStudio)
    );
}

#[test]
fn complete_chat_round_trips_against_local_openai_compatible_server() {
    let server = tiny_http::Server::http("127.0.0.1:0").expect("bind ephemeral port");
    let endpoint = format!("http://{}", server.server_addr().to_ip().unwrap());

    // Run a single-shot mini-server in a worker thread. We surface the request
    // body so the test can assert on what the harness sent.
    let (tx, rx) = mpsc::channel::<serde_json::Value>();

    let server_handle = thread::spawn(move || {
        let mut request = server.recv().expect("server received request");

        let mut body = String::new();
        request
            .as_reader()
            .read_to_string(&mut body)
            .expect("request body utf8");

        let parsed: serde_json::Value = serde_json::from_str(&body).expect("body is JSON");
        tx.send(parsed).expect("send parsed body");

        let response_body = serde_json::json!({
            "id": "test-1",
            "model": "qwen2.5-coder:14b",
            "choices": [
                {
                    "index": 0,
                    "message": {"role": "assistant", "content": "pong"},
                    "finish_reason": "stop"
                }
            ],
            "usage": {
                "prompt_tokens": 4,
                "completion_tokens": 1,
                "total_tokens": 5
            }
        })
        .to_string();
        let response_bytes = response_body.into_bytes();
        let len = response_bytes.len();
        let response = tiny_http::Response::new(
            tiny_http::StatusCode(200),
            vec![
                tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                    .unwrap(),
            ],
            Cursor::new(response_bytes),
            Some(len),
            None,
        );
        request.respond(response).expect("respond");
    });

    let options = ChatCompletionOptions {
        provider: InferenceProvider::Ollama,
        endpoint: endpoint.clone(),
        model_key_or_id: "qwen2.5-coder-14b".to_string(),
        messages: vec![
            ChatMessage::system("You are a tester."),
            ChatMessage::user("ping"),
        ],
        temperature: Some(0.0),
        max_tokens: Some(16),
        top_p: None,
        timeout: Duration::from_secs(5),
    };

    let response = complete_chat(&options).expect("chat completion succeeds");
    server_handle.join().expect("server thread joined");

    assert_eq!(response.first_text(), Some("pong"));
    assert_eq!(response.choices.len(), 1);
    assert_eq!(response.choices[0].finish_reason.as_deref(), Some("stop"));
    let usage = response.usage.expect("usage present");
    assert_eq!(usage.total_tokens, Some(5));

    let request_body = rx.recv().expect("received request body");

    // Registry key must be resolved to the Ollama-style model id over the wire.
    assert_eq!(
        request_body
            .get("model")
            .and_then(serde_json::Value::as_str),
        Some("qwen2.5-coder:14b")
    );
    let messages = request_body
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .expect("messages array");
    assert_eq!(messages.len(), 2);
    assert_eq!(
        messages[0].get("role").and_then(|v| v.as_str()),
        Some("system")
    );
    assert_eq!(
        messages[1].get("role").and_then(|v| v.as_str()),
        Some("user")
    );
    assert_eq!(
        messages[1].get("content").and_then(|v| v.as_str()),
        Some("ping")
    );
    assert_eq!(
        request_body
            .get("stream")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
}

#[test]
fn complete_chat_surfaces_non_2xx_responses() {
    let server = tiny_http::Server::http("127.0.0.1:0").expect("bind ephemeral port");
    let endpoint = format!("http://{}", server.server_addr().to_ip().unwrap());

    let server_handle = thread::spawn(move || {
        let request = server.recv().expect("recv");
        let body = b"model not found".to_vec();
        let len = body.len();
        let response = tiny_http::Response::new(
            tiny_http::StatusCode(404),
            vec![],
            Cursor::new(body),
            Some(len),
            None,
        );
        request.respond(response).expect("respond");
    });

    let options = ChatCompletionOptions {
        provider: InferenceProvider::LmStudio,
        endpoint,
        model_key_or_id: "qwen2.5-coder-14b".to_string(),
        messages: vec![ChatMessage::user("ping")],
        temperature: Some(0.0),
        max_tokens: None,
        top_p: None,
        timeout: Duration::from_secs(5),
    };

    let err = complete_chat(&options).expect_err("404 should error");
    let chain = format!("{err:#}");
    assert!(
        chain.contains("HTTP 404"),
        "expected error to mention HTTP 404, got: {chain}"
    );
    server_handle.join().expect("server thread joined");
}

#[test]
fn resolve_provider_model_id_falls_back_for_unknown_keys() {
    // unknown registry key is None — caller is expected to pass it through raw
    assert!(resolve_provider_model_id(InferenceProvider::Ollama, "made-up-model").is_none());
    assert!(find_model("made-up-model").is_none());
}
