use numera_image_gen_mcp::compat::value::Value;
use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: String,
    pub body: Vec<u8>,
}

pub struct Reply {
    pub status: u16,
    pub mime: &'static str,
    pub body: Vec<u8>,
}

pub fn json_reply(value: Value) -> Reply {
    Reply {
        status: 200,
        mime: "application/json",
        body: value.to_string().into_bytes(),
    }
}

pub(super) fn find(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes
        .windows(needle.len())
        .position(|chunk| chunk == needle)
}

async fn capture(socket: &mut TcpStream) -> Option<Request> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let read = match socket.read(&mut buffer).await {
            Ok(0) => return None,
            Ok(read) => read,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                ) =>
            {
                return None;
            }
            Err(error) => panic!("Fixture socket read failed: {error}"),
        };
        bytes.extend_from_slice(&buffer[..read]);
        assert!(bytes.len() <= 1048576 + 65536, "bounded fixture request");
        let Some(boundary) = find(&bytes, b"\r\n\r\n") else {
            assert!(bytes.len() <= 65536, "bounded fixture headers");
            continue;
        };
        assert!(boundary <= 65536, "bounded fixture headers");
        let headers = std::str::from_utf8(&bytes[..boundary]).unwrap();
        let length = header(headers, "content-length")
            .map(|value| value.parse::<usize>().unwrap())
            .unwrap_or(0);
        assert!(length <= 1048576, "fixture body exceeds one MiB");
        if bytes.len() < boundary + 4 + length {
            continue;
        }
        assert_eq!(bytes.len(), boundary + 4 + length);
        let mut first = headers.lines().next().unwrap().split_whitespace();
        return Some(Request {
            method: first.next().unwrap().to_owned(),
            path: first.next().unwrap().to_owned(),
            headers: headers.to_owned(),
            body: bytes[boundary + 4..].to_vec(),
        });
    }
}

pub async fn serve<F, A>(mut respond: impl FnMut(&Request) -> Reply, action: F)
where
    F: FnOnce(String, Arc<Mutex<Vec<Request>>>) -> A,
    A: Future<Output = ()>,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let server = async {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            tokio::time::timeout(Duration::from_secs(3), async {
                let Some(request) = capture(&mut socket).await else {
                    return;
                };
                let response = respond(&request);
                captured.lock().unwrap().push(request);
                let headers = format!(
                    "HTTP/1.1 {} Fixture\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response.status,
                    response.mime,
                    response.body.len()
                );
                if socket.write_all(headers.as_bytes()).await.is_ok()
                    && socket.write_all(&response.body).await.is_ok()
                {
                    let _ = socket.shutdown().await;
                }
            })
            .await
            .expect("Fixture socket exceeded three seconds");
        }
    };
    // Scoped futures drop every socket/listener on success, panic and timeout.
    tokio::time::timeout(Duration::from_secs(8), async {
        tokio::select! {
            () = action(origin, requests) => {},
            () = server => panic!("Fixture listener ended unexpectedly"),
        }
    })
    .await
    .expect("Wire acceptance action exceeded eight seconds");
}

pub(super) fn header<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    headers.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then(|| value.trim())
    })
}
