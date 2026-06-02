use std::time::Duration;

use axum::routing::get;
use mutagen_service::app;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

async fn boot(
    router: axum::Router,
) -> (
    std::net::SocketAddr,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral");
    let addr = listener.local_addr().expect("local_addr");
    let (tx, rx) = oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await
            .expect("serve");
    });
    (addr, tx, handle)
}

#[tokio::test]
async fn health_returns_ok() {
    let (addr, tx, handle) = boot(app()).await;

    let body: serde_json::Value = reqwest::get(format!("http://{addr}/health"))
        .await
        .expect("GET /health")
        .json()
        .await
        .expect("decode json");

    assert_eq!(body["status"], "ok");

    tx.send(()).expect("trigger shutdown");
    handle.await.expect("server task");
}

#[tokio::test]
async fn shutdown_drains_inflight() {
    let router = app().route(
        "/slow",
        get(|| async {
            tokio::time::sleep(Duration::from_millis(400)).await;
            "done"
        }),
    );
    let (addr, tx, handle) = boot(router).await;

    let client = reqwest::Client::new();
    let url = format!("http://{addr}/slow");
    let req = tokio::spawn(async move {
        let resp = client.get(url).send().await.expect("send /slow");
        let status = resp.status();
        let body = resp.text().await.expect("read body");
        (status, body)
    });

    tokio::time::sleep(Duration::from_millis(75)).await;
    tx.send(()).expect("trigger shutdown mid-request");

    let (status, body) = req.await.expect("inflight request task");
    assert_eq!(status.as_u16(), 200, "in-flight request was not drained");
    assert_eq!(body, "done");

    handle.await.expect("server task");
}
