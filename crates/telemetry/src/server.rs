//! Minimal hyper-based /metrics endpoint.

use crate::metrics::MetricsRegistry;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing::{debug, info};

/// Bind a TCP listener on `addr` and serve `/metrics`. Runs until cancelled.
pub async fn serve_metrics(addr: SocketAddr, metrics: MetricsRegistry) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    info!(%addr, "metrics endpoint listening");

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let m = metrics.clone();
        tokio::spawn(async move {
            let svc = service_fn(move |req| handle(req, m.clone()));
            if let Err(err) = hyper::server::conn::http1::Builder::new()
                .serve_connection(io, svc)
                .await
            {
                debug!(error = %err, "connection closed");
            }
        });
    }
}

async fn handle(
    req: Request<hyper::body::Incoming>,
    metrics: MetricsRegistry,
) -> Result<Response<Full<Bytes>>, Infallible> {
    match req.uri().path() {
        "/metrics" => {
            let body = metrics.encode().await;
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header(
                    "content-type",
                    "application/openmetrics-text; version=1.0.0; charset=utf-8",
                )
                .body(Full::new(Bytes::from(body)))
                .unwrap())
        }
        "/health" => Ok(Response::builder()
            .status(StatusCode::OK)
            .body(Full::new(Bytes::from_static(b"ok")))
            .unwrap()),
        _ => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Full::new(Bytes::from_static(b"not found")))
            .unwrap()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn server_returns_metrics_on_path() {
        let addr: SocketAddr = ([127, 0, 0, 1], 0).into();
        let listener = TcpListener::bind(addr).await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let metrics = MetricsRegistry::new();
        metrics
            .polls_total
            .get_or_create(&crate::metrics::InverterLabels {
                slot: "test".into(),
            })
            .inc();
        let m_clone = metrics.clone();

        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let io = TokioIo::new(stream);
                let m = m_clone.clone();
                tokio::spawn(async move {
                    let svc = service_fn(move |req| handle(req, m.clone()));
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, svc)
                        .await;
                });
            }
        });

        // tiny sleep for binding
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let resp = reqwest::get(format!("http://127.0.0.1:{port}/metrics"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.text().await.unwrap();
        assert!(body.contains("sma_polls_total"));
        assert!(body.contains(r#"slot="test""#));
    }

    #[tokio::test]
    async fn health_endpoint() {
        let addr: SocketAddr = ([127, 0, 0, 1], 0).into();
        let listener = TcpListener::bind(addr).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let metrics = MetricsRegistry::new();
        let m = metrics.clone();
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let io = TokioIo::new(stream);
                let m = m.clone();
                tokio::spawn(async move {
                    let svc = service_fn(move |req| handle(req, m.clone()));
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, svc)
                        .await;
                });
            }
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let resp = reqwest::get(format!("http://127.0.0.1:{port}/health"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert_eq!(resp.text().await.unwrap(), "ok");
    }
}
