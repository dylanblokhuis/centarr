use std::net::SocketAddr;
use std::os::unix::prelude::AsRawFd;
use std::path::PathBuf;

use axum::http::{HeaderMap, HeaderValue, Request};
use nix::errno::Errno;
use regex::Regex;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};
use tokio::net::{TcpListener, TcpStream};

static CHUNK_SIZE: i64 = 1_048_576;

fn parse_request(buf: &[u8]) -> Option<Request<()>> {
    let string = String::from_utf8(buf.to_vec()).unwrap();
    let mut request = Request::builder();
    let mut complete = false;

    for raw_line in string.lines() {
        let line = raw_line.replace('\0', "");

        if line.contains("HTTP/1.1") {
            let mut parts = line.split(' ');
            let method = parts.next().unwrap();
            let uri = parts.next().unwrap();

            request = request.method(method).uri(uri);
            continue;
        }

        if line.contains(':') {
            let mut parts = line.split(": ");
            let key = parts.next().unwrap();
            let value = parts.next().unwrap();

            let maybe_valid_header = HeaderValue::from_str(value);
            if let Ok(valid_header) = maybe_valid_header {
                request = request.header(key, valid_header);
            }
            continue;
        }

        if line.is_empty() {
            complete = true;
        }
    }

    if !complete {
        return None;
    }

    Some(request.body(()).unwrap())
}

async fn get_request_from_stream(socket: &mut TcpStream) -> Request<()> {
    let mut req = None;
    let mut buf = vec![0; 1024];
    let mut writer = BufWriter::new(&mut buf);

    for _ in 1..5 {
        let mut temp_buf = vec![0; 1024];
        socket.read_buf(&mut temp_buf).await.unwrap();
        writer.write_all(&temp_buf).await.unwrap();

        let maybe_req = parse_request(writer.buffer());

        if let Some(inner) = maybe_req {
            req = Some(inner);
            break;
        }
    }

    if req.is_none() {
        panic!("Could not parse request");
    }

    req.unwrap()
}

pub async fn server() {
    let addr = "0.0.0.0:3001".parse::<SocketAddr>().unwrap();

    let listener = TcpListener::bind(&addr).await.unwrap();
    println!("Listening on: {}", addr);

    loop {
        let (mut stream, _) = listener.accept().await.unwrap();

        tokio::spawn(async move {
            let req = get_request_from_stream(&mut stream).await;

            let mut range = "bytes=0-";
            let maybe_range_header = req
                .headers()
                .iter()
                .find(|(name, _)| name == &axum::http::header::RANGE);
            if let Some((_, value)) = maybe_range_header {
                range = value.to_str().unwrap();
            }

            let path_encoded = req.uri().to_string().replace("/?file=", "");
            let path_decoded = urlencoding::decode(path_encoded.as_str()).unwrap();
            let filename = PathBuf::from(path_decoded.to_string());

            let file = tokio::fs::OpenOptions::new()
                .read(true)
                .write(false)
                .open(&filename)
                .await
                .unwrap();
            let metadata = file.metadata().await.unwrap();
            let mut start_index;
            let mut end_index = metadata.len() as i64 - 1;

            let captures = Regex::new(r"bytes=(\d+)-(\d+)?")
                .unwrap()
                .captures(range)
                .unwrap();
            let start = captures.get(1).unwrap().as_str();
            start_index = start.parse::<i64>().unwrap();

            if let Some(end) = captures.get(2) {
                end_index = end.as_str().parse::<i64>().unwrap();
            }

            stream
                .write_all(b"HTTP/1.1 206 Partial Content\r\n".as_slice())
                .await
                .unwrap();

            let mut headers = HeaderMap::new();
            headers.append("Accept-Ranges", HeaderValue::from_static("bytes"));
            headers.append(
                "Content-Type",
                HeaderValue::from_static("application/octet-stream"),
            );
            headers.append(
                "Content-Range",
                HeaderValue::from_str(
                    format!("bytes {}-{}/{}", start_index, end_index, metadata.len()).as_str(),
                )
                .unwrap(),
            );
            headers.append("Connection", HeaderValue::from_static("close"));
            headers.append(
                "Content-Length",
                HeaderValue::from_str((end_index - start_index + 1).to_string().as_str()).unwrap(),
            );

            for (name, value) in headers {
                let bytes = format!("{}: {}\r\n", name.unwrap(), value.to_str().unwrap());
                stream.write_all(bytes.as_bytes()).await.unwrap();
            }

            stream.write_all(b"\r\n").await.unwrap();

            println!("starting from {} to {}", start_index, end_index);

            let mut bytes_read: i64 = 0;
            loop {
                let chunk_size = std::cmp::min(CHUNK_SIZE, (end_index + 1) - bytes_read);

                if chunk_size == 0 {
                    break;
                }

                match nix::sys::sendfile::sendfile(
                    stream.as_raw_fd(),
                    file.as_raw_fd(),
                    Some(&mut start_index),
                    chunk_size as usize,
                ) {
                    Ok(bytes) => {
                        bytes_read += bytes as i64;
                    }
                    Err(e) => {
                        if e != Errno::EAGAIN {
                            println!("sendfile(2) error {:?}", e);
                            break;
                        }
                    }
                }
            }

            let mut buffer = Vec::new();
            stream.read_to_end(&mut buffer).await.unwrap();
            println!("closing stream");
        });
    }
}
