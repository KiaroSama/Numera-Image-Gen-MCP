use numera_image_gen_mcp::compat::value::Value;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[derive(Clone)]
pub struct Reply {
    pub status: u16,
    pub value: Value,
    pub bytes: Option<Vec<u8>>,
}

pub struct Server {
    pub origin: String,
    pub requests: Arc<Mutex<Vec<String>>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl Server {
    pub async fn new(reply: impl Fn(&str) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let action = async {
                    let mut bytes = Vec::new();
                    let mut buffer = [0u8; 4096];
                    let boundary = loop {
                        let n = socket.read(&mut buffer).await.unwrap();
                        assert!(n > 0, "fixture request ended before headers");
                        bytes.extend_from_slice(&buffer[..n]);
                        assert!(bytes.len() <= 65536, "fixture request bound");
                        if let Some(boundary) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                            let headers = String::from_utf8_lossy(&bytes[..boundary]);
                            let length = headers
                                .lines()
                                .find_map(|line| {
                                    line.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .and_then(|value| value.trim().parse::<usize>().ok())
                                })
                                .unwrap_or(0);
                            if bytes.len() >= boundary + 4 + length {
                                break boundary;
                            }
                        }
                    };
                    let headers = String::from_utf8(bytes[..boundary].to_vec()).unwrap();
                    let path = headers
                        .lines()
                        .next()
                        .unwrap()
                        .split_whitespace()
                        .nth(1)
                        .unwrap();
                    captured.lock().unwrap().push(path.to_owned());
                    let response = reply(path);
                    let body = response
                        .bytes
                        .unwrap_or_else(|| response.value.to_string().into_bytes());
                    let wire = format!(
                        "HTTP/1.1 {} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        response.status,
                        body.len()
                    );
                    if socket.write_all(wire.as_bytes()).await.is_ok()
                        && socket.write_all(&body).await.is_ok()
                    {
                        let _ = socket.shutdown().await;
                    }
                };
                tokio::time::timeout(Duration::from_secs(3), action)
                    .await
                    .unwrap();
            }
        });
        Self {
            origin,
            requests,
            task: Some(task),
        }
    }

    pub async fn close(mut self) {
        let task = self.task.take().unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
