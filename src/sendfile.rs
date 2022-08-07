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
            socket.read(&mut buf).await.unwrap();

            let mut headers = [httparse::EMPTY_HEADER; 16];
            let mut req = httparse::Request::new(&mut headers);
            req.parse(&mut buf).unwrap();

            let range = String::from_utf8(
                req.headers
                    .iter()
                    .find(|h| h.name == "Range")
                    .unwrap()
                    .value
                    .into(),
            )
            .unwrap();

            let path_encoded = req.path.unwrap().replace("/?file=", "");
            let path_decoded = urlencoding::decode(path_encoded.as_str()).unwrap();
            let filename = PathBuf::from(path_decoded.to_string());

            let file = std::fs::File::open(&filename).unwrap();
            let metadata = file.metadata().unwrap();

            let mut start_index;
            let mut end_index: i64 = 0;

            let re = Regex::new(r"bytes=(\d+)-(\d+)?").unwrap();
            let captures = re.captures(range.as_str()).unwrap();
            let start = captures.get(1).unwrap().as_str();
            start_index = start.parse::<i64>().unwrap();

            if let Some(end) = captures.get(2) {
                end_index = end.as_str().parse::<i64>().unwrap();
            }

            if start_index == 0 && end_index == 0 {
                end_index = metadata.len() as i64;
            }

            if start_index != 0 && end_index == 0 {
                end_index = metadata.len() as i64;
            }

            let read_amount = end_index - start_index;

            socket
                .write_all(&mut b"HTTP/1.1 206 Partial Content\r\n".as_slice())
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
                socket.write_all(&mut bytes.as_bytes()).await.unwrap();
            }

            socket.write_all(b"\r\n").await.unwrap();

            // println!("start_index: {}, count: {}", start_index, read_amount);

            let mut bytes_read: usize = 0;
            while bytes_read != read_amount as usize {
                let chunk_size = std::cmp::min(CHUNK_SIZE, end_index as u64 - bytes_read as u64);

                // println!("start_index {}: chunk_size: {}", start_index, chunk_size);
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

                        socket.writable().await.unwrap();
                        // socket.flush().await.unwrap();
                        // println!("{:?}", res);
                        // std::thread::sleep(std::time::Duration::from_millis(10));
                        // break;
                    }
                }
                // println!("bytes_sent: {}", bytes_read);
            }
        });
    }
}
