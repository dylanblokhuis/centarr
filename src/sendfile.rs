use std::net::SocketAddr;
use std::os::unix::prelude::AsRawFd;
use std::path::PathBuf;
use std::time::SystemTime;

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
        let (mut stream, addr) = listener.accept().await.unwrap();

        tokio::spawn(async move {
            process(&mut stream, addr).await;
        });
    }
}

pub async fn process(stream: &mut TcpStream, addr: SocketAddr) {
    let req = get_request_from_stream(stream).await;
    println!("{:?} Parsed request", addr);

    let mut range = "bytes=0-";
    let maybe_range_header = req
        .headers()
        .iter()
        .find(|(name, _)| name == &axum::http::header::RANGE);
    if let Some((_, value)) = maybe_range_header {
        range = value.to_str().unwrap();
    }

    println!("{:?} Has range: {:?}", addr, range);

    let path_encoded = req.uri().to_string().replace("/?file=", "");
    let path_decoded = urlencoding::decode(path_encoded.as_str()).unwrap();
    let filename = PathBuf::from(path_decoded.to_string());

    println!("{:?} Opening file: {:?}", addr, filename);

    let file = tokio::fs::OpenOptions::new()
        .read(true)
        .write(false)
        .open(&filename)
        .await
        .unwrap();
    println!("{:?} Opened file {:?}", addr, filename);
    let metadata = file.metadata().await.unwrap();
    let mut start_index;
    let mut end_index = metadata.len() as i64;

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
    headers.append("Server", HeaderValue::from_static("centarr"));
    headers.append(
        "Date",
        HeaderValue::from_str(httpdate::fmt_http_date(SystemTime::now()).as_str()).unwrap(),
    );
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
    if let Some(header) = req.headers().get("Connection") {
        if header.to_str().unwrap().to_lowercase() == "keep-alive" {
            headers.append("Connection", HeaderValue::from_static("close"));
        }
    }
    headers.append(
        "Content-Length",
        HeaderValue::from_str((end_index - start_index).to_string().as_str()).unwrap(),
    );

    for (name, value) in headers {
        let bytes = format!("{}: {}\r\n", name.unwrap(), value.to_str().unwrap());
        stream.write_all(bytes.as_bytes()).await.unwrap();
    }

    stream.write_all(b"\r\n").await.unwrap();

    println!("{:?} Starting from {} to {}", addr, start_index, end_index);

    let mut completed = false;
    let mut bytes_read: i64 = start_index;
    let stream_fd = stream.as_raw_fd();
    let file_fd = file.as_raw_fd();

    loop {
        let mut offset = start_index;
        let chunk_size = std::cmp::min(CHUNK_SIZE, end_index - bytes_read);
        let result = tokio::spawn(async move {
            nix::sys::sendfile::sendfile(stream_fd, file_fd, Some(&mut offset), chunk_size as usize)
        });

        let res = result.await.unwrap();
        if let Ok(bytes) = res {
            println!("{:?} Start index: {}", addr, start_index);
            println!("{:?} Read bytes: {}", addr, bytes);

            if bytes == 0 {
                completed = true;
                break;
            }
            bytes_read += bytes as i64;
            start_index = bytes_read
        }

        if let Err(e) = res {
            // println!("{:?} Error: {:?}", addr, e);
            if e != Errno::EAGAIN {
                break;
            }
        }
    }

    if completed {
        println!("{:?} waiting for socket to end", addr);
        let mut buffer = Vec::new();
        stream.read_to_end(&mut buffer).await.unwrap();
    }

    stream.flush().await.unwrap();
    println!("{:?} Closing stream", addr);
}
