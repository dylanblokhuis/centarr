use std::net::SocketAddr;
use std::os::unix::prelude::AsRawFd;
use std::path::PathBuf;

use axum::http::{HeaderMap, HeaderValue};
use nix::errno::Errno;
use regex::Regex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

static CHUNK_SIZE: u64 = 65536;
pub async fn server() {
    let addr = "0.0.0.0:3001".parse::<SocketAddr>().unwrap();

    let listener = TcpListener::bind(&addr).await.unwrap();
    println!("Listening on: {}", addr);

    loop {
        let (mut socket, _) = listener.accept().await.unwrap();

        tokio::spawn(async move {
            let mut buf = vec![0; 1024];
            socket.read_exact(&mut buf).await.unwrap();

            println!("{}", String::from_utf8_lossy(&buf));

            let mut headers = [httparse::EMPTY_HEADER; 64];
            let mut req = httparse::Request::new(&mut headers);
            req.parse(&buf).unwrap();

            let mut range = "bytes=0-".to_string();
            let maybe_range_header = req.headers.iter().find(|h| h.name == "Range");
            if let Some(range_header) = maybe_range_header {
                range = String::from_utf8(range_header.value.into()).unwrap();
            }

            let path_encoded = req.path.unwrap().replace("/?file=", "");
            let path_decoded = urlencoding::decode(path_encoded.as_str()).unwrap();
            let filename = PathBuf::from(path_decoded.to_string());

            let file = std::fs::File::open(&filename).unwrap();
            let metadata = file.metadata().unwrap();

            let mut start_index;
            let mut end_index = metadata.len() as i64;

            let captures = Regex::new(r"bytes=(\d+)-(\d+)?")
                .unwrap()
                .captures(range.as_str())
                .unwrap();
            let start = captures.get(1).unwrap().as_str();
            start_index = start.parse::<i64>().unwrap();

            if let Some(end) = captures.get(2) {
                end_index = end.as_str().parse::<i64>().unwrap();
            }

            let read_amount = end_index - start_index;

            socket
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
            headers.append(
                "Content-Length",
                HeaderValue::from_str(read_amount.to_string().as_str()).unwrap(),
            );

            for (name, value) in headers {
                let bytes = format!("{}: {}\r\n", name.unwrap(), value.to_str().unwrap());
                socket.write_all(bytes.as_bytes()).await.unwrap();
            }

            socket.write_all(b"\r\n").await.unwrap();

            let mut bytes_read: usize = 0;
            while bytes_read != read_amount as usize {
                let chunk_size = std::cmp::min(CHUNK_SIZE, end_index as u64 - bytes_read as u64);

                match nix::sys::sendfile::sendfile(
                    socket.as_raw_fd(),
                    file.as_raw_fd(),
                    Some(&mut start_index),
                    chunk_size as usize,
                ) {
                    Ok(bytes) => {
                        if bytes == 0 {
                            println!("Connection lost");
                            return;
                        }

                        bytes_read += bytes;
                    }
                    Err(e) => {
                        if e != Errno::EAGAIN {
                            println!("sendfile(2) error {:?}", e);
                            break;
                        }

                        break;
                    }
                }
            }
        });
    }
}
