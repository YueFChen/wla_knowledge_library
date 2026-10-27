use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, atomic::AtomicUsize},
    time::{Duration, Instant},
};
fn service(kind: &str) -> Service {
    Service {
        id: "local".into(),
        name: "Local".into(),
        kind: kind.into(),
        url: "http://127.0.0.1:1234".into(),
        secret_id: None,
        revision: "1".into(),
    }
}
fn model(kind: &str) -> ResolvedModel {
    ResolvedModel {
        service: service(kind),
        profile: ModelProfile {
            id: "profile".into(),
            name: "User choice".into(),
            service_id: "local".into(),
            model: "arbitrary/new-embedding:latest".into(),
            dimensions: 3,
            max_chars: 64,
            query_prefix: "query: ".into(),
            document_prefix: "passage: ".into(),
            revision: "1".into(),
        },
    }
}
struct Fake {
    calls: Mutex<Vec<Value>>,
    bad: Option<Value>,
    single: bool,
}
impl Transport for Fake {
    fn request(
        &self,
        service: &Service,
        _: &str,
        _: &str,
        body: Option<&Value>,
        _: &AtomicBool,
    ) -> Result<Value> {
        let body = body.unwrap();
        self.calls.lock().unwrap().push(body.clone());
        if let Some(bad) = &self.bad {
            return Ok(bad.clone());
        }
        let inputs = body["input"]
            .as_array()
            .cloned()
            .unwrap_or_else(|| vec![body["input"].clone()]);
        if self.single && inputs.len() > 1 {
            return Err("模型服务 HTTP 422".into());
        }
        Ok(if service.kind == "ollama" {
            json!({"embeddings":inputs.iter().map(|_|vec![1.0,2.0,3.0]).collect::<Vec<_>>()})
        } else {
            json!({"data":inputs.iter().enumerate().rev().map(|(index,_)|json!({"index":index,"embedding":[1.0,2.0,3.0]})).collect::<Vec<_>>()})
        })
    }
}
#[test]
fn protocols_accept_arbitrary_models_reordered_batches_and_preserve_all_unicode_input() {
    for kind in ["openai", "ollama"] {
        let fake = Fake {
            calls: Mutex::new(Vec::new()),
            bad: None,
            single: false,
        };
        let resolved = model(kind);
        let text = "取消任务后保留完整的资料快照。".repeat(30);
        let result = ModelPool::default()
            .embed(
                &resolved,
                &[text.clone(), "English source text".into()],
                false,
                &fake,
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].len(), 3);
        let calls = fake.calls.lock().unwrap();
        let mut parts = Vec::new();
        for body in calls.iter() {
            assert_eq!(body["model"], resolved.profile.model);
            if kind == "ollama" {
                assert_eq!(body["truncate"], false);
            }
            let inputs = body["input"]
                .as_array()
                .cloned()
                .unwrap_or_else(|| vec![body["input"].clone()]);
            for input in inputs {
                let input = input.as_str().unwrap();
                assert!(input.chars().count() <= 64);
                parts.push(input.strip_prefix("passage: ").unwrap().to_string());
            }
        }
        assert_eq!(parts.join(""), format!("{text}English source text"));
    }
}
#[test]
fn single_input_fallback_and_query_cache_are_configuration_scoped() {
    let fake = Fake {
        calls: Mutex::new(Vec::new()),
        bad: None,
        single: true,
    };
    let mut resolved = model("openai");
    let pool = ModelPool::default();
    let cancelled = AtomicBool::new(false);
    assert_eq!(
        pool.embed(&resolved, &["a".repeat(200)], false, &fake, &cancelled)
            .unwrap()
            .len(),
        1
    );
    pool.query(&resolved, "hello", &fake, &cancelled).unwrap();
    let count = fake.calls.lock().unwrap().len();
    pool.query(&resolved, "hello", &fake, &cancelled).unwrap();
    assert_eq!(fake.calls.lock().unwrap().len(), count);
    resolved.service.revision = "changed".into();
    pool.query(&resolved, "hello", &fake, &cancelled).unwrap();
    assert_eq!(fake.calls.lock().unwrap().len(), count + 1);
    cancelled.store(true, Ordering::Release);
    assert!(
        pool.embed(&resolved, &["text".into()], false, &fake, &cancelled)
            .is_err()
    );
}
#[test]
fn malformed_embedding_results_are_rejected_instead_of_indexed() {
    for bad in [
        json!({"data":[]}),
        json!({"data":[{"index":0,"embedding":[]}]}),
        json!({"data":[{"index":0,"embedding":[0,0,0]}]}),
        json!({"data":[{"index":1,"embedding":[1,2,3]}]}),
        json!({"data":[{"index":0,"embedding":[1,2]}]}),
        json!({"data":[{"index":0,"embedding":["NaN",2,3]}]}),
    ] {
        let fake = Fake {
            calls: Mutex::new(Vec::new()),
            bad: Some(bad),
            single: false,
        };
        assert!(
            ModelPool::default()
                .embed(
                    &model("openai"),
                    &["hello".into()],
                    false,
                    &fake,
                    &AtomicBool::new(false)
                )
                .is_err()
        );
    }
    assert!(normalize(vec![f32::NAN], 0).is_err());
    assert!(normalize(vec![f32::INFINITY], 0).is_err());
}
#[test]
fn only_local_services_and_own_credential_identity_are_accepted() {
    for url in [
        "https://example.com/v1",
        "http://192.168.1.2:1234",
        "http://user:secret@localhost",
        "http://localhost/v1?key=secret",
        "file:///tmp/model",
    ] {
        let mut s = service("openai");
        s.url = url.into();
        assert!(s.validate().is_err());
    }
    for url in [
        "http://localhost:1234",
        "http://[::1]:1234/v1",
        "http://127.0.0.2:1234",
    ] {
        let mut s = service("openai");
        s.url = url.into();
        s.validate().unwrap();
    }
    assert_eq!(
        service("openai").endpoint("embeddings").unwrap(),
        "http://127.0.0.1:1234/v1/embeddings"
    );
    assert_eq!(
        service("ollama").endpoint("embed").unwrap(),
        "http://127.0.0.1:1234/api/embed"
    );
    let mut s = service("openai");
    s.secret_id = Some("../another-plugin".into());
    assert!(s.validate().is_err());
    assert!(secret_root(Path::new("/tmp/other-plugin")).is_none());
}
fn server(response: &str, delay: Duration) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let response = response.to_owned();
    let thread = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0; 2048];
        loop {
            let n = stream.read(&mut buffer).unwrap_or(0);
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..n]);
            if let Some(pos) = bytes.windows(4).position(|x| x == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&bytes[..pos]);
                let length = header
                    .lines()
                    .find_map(|line| {
                        line.to_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|n| n.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if bytes.len() >= pos + 4 + length {
                    break;
                }
            }
        }
        std::thread::sleep(delay);
        let _ = stream.write_all(response.as_bytes());
        String::from_utf8_lossy(&bytes).into_owned()
    });
    (format!("http://{addr}"), thread)
}
#[test]
fn independent_native_http_supports_model_listing_and_refuses_redirects() {
    let body = r#"{"data":[{"id":"new-model"}]}"#;
    let (url, thread) = server(
        &format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
        Duration::ZERO,
    );
    let mut s = service("openai");
    s.url = url;
    let root = std::env::temp_dir().join(crate::workspace::unique());
    assert_eq!(
        list_models(&s, &NativeTransport(&root), &AtomicBool::new(false)).unwrap(),
        vec!["new-model"]
    );
    assert!(thread.join().unwrap().starts_with("GET /v1/models"));
    assert!(!root.exists());
    let (url, thread) = server(
        "HTTP/1.1 302 Found\r\nLocation: https://example.com/secret\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        Duration::ZERO,
    );
    s.url = url;
    assert_eq!(
        list_models(&s, &NativeTransport(&root), &AtomicBool::new(false)).unwrap_err(),
        "模型服务 HTTP 302"
    );
    thread.join().unwrap();
}
#[test]
fn independent_native_http_can_cancel_while_waiting_for_a_model() {
    let (url, thread) = server(
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}",
        Duration::from_secs(1),
    );
    let mut s = service("openai");
    s.url = url;
    let token = Arc::new(AtomicBool::new(false));
    let other = token.clone();
    let count = Arc::new(AtomicUsize::new(0));
    let observed = count.clone();
    let cancel = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        other.store(true, Ordering::Release);
        observed.store(1, Ordering::Release)
    });
    let start = Instant::now();
    assert_eq!(
        list_models(&s, &NativeTransport(Path::new("unused")), &token).unwrap_err(),
        "任务已取消"
    );
    assert!(start.elapsed() < Duration::from_millis(900));
    cancel.join().unwrap();
    assert_eq!(count.load(Ordering::Acquire), 1);
    thread.join().unwrap();
}
#[cfg(windows)]
#[test]
fn standalone_credentials_are_read_only_and_scoped_to_this_plugin() {
    let root = std::env::temp_dir().join(crate::workspace::unique());
    let cache = root.join("plugin-data").join(PLUGIN_ID);
    let secrets = root.join("plugin-secrets").join(PLUGIN_ID);
    std::fs::create_dir_all(&secrets).unwrap();
    let mut s = service("openai");
    s.secret_id = Some("embedding_local".into());
    let path = secrets.join("embedding_local.dpapi");
    let sealed = wonderland_secret::seal(b"synthetic-local-test-key").unwrap();
    std::fs::write(&path, &sealed).unwrap();
    assert_eq!(
        read_credential(&cache, &s).unwrap().as_deref(),
        Some("synthetic-local-test-key")
    );
    assert_eq!(std::fs::read(&path).unwrap(), sealed);
    assert!(!cache.exists());
    s.id = "another".into();
    assert!(read_credential(&cache, &s).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
