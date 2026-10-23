use crate::wire::{Request, find, header};

pub struct Part<'a> {
    pub name: &'a str,
    pub filename: Option<&'a str>,
    pub mime: Option<&'a str>,
    pub bytes: &'a [u8],
}

fn quoted<'a>(disposition: &'a str, key: &str) -> Option<&'a str> {
    disposition.split(';').find_map(|item| {
        let (name, value) = item.trim().split_once('=')?;
        (name == key).then(|| value.strip_prefix('"').unwrap().strip_suffix('"').unwrap())
    })
}

pub fn multipart(request: &Request) -> Vec<Part<'_>> {
    let content_type = header(&request.headers, "content-type").unwrap();
    assert!(content_type.starts_with("multipart/form-data;"));
    let boundary = content_type
        .split(';')
        .find_map(|item| item.trim().strip_prefix("boundary="))
        .unwrap()
        .trim_matches('"');
    assert!(!boundary.is_empty());
    let delimiter = format!("--{boundary}");
    assert!(
        request
            .body
            .starts_with(format!("{delimiter}\r\n").as_bytes())
    );
    let mut offset = delimiter.len() + 2;
    let mut parts = Vec::new();
    loop {
        let end = offset + find(&request.body[offset..], b"\r\n\r\n").unwrap();
        let headers = std::str::from_utf8(&request.body[offset..end]).unwrap();
        let disposition = header(headers, "content-disposition").unwrap();
        let start = end + 4;
        let next = start
            + find(
                &request.body[start..],
                format!("\r\n{delimiter}").as_bytes(),
            )
            .unwrap();
        parts.push(Part {
            name: quoted(disposition, "name").unwrap(),
            filename: quoted(disposition, "filename"),
            mime: header(headers, "content-type"),
            bytes: &request.body[start..next],
        });
        assert!(parts.len() <= 16, "bounded multipart part count");
        offset = next + 2 + delimiter.len();
        if request.body.get(offset..offset + 2) == Some(b"--".as_slice()) {
            let tail = &request.body[offset + 2..];
            assert!(tail.is_empty() || tail == b"\r\n");
            return parts;
        }
        assert_eq!(
            request.body.get(offset..offset + 2),
            Some(b"\r\n".as_slice())
        );
        offset += 2;
    }
}
