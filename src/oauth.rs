use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, TcpListener},
    thread,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::{Digest, Sha256};
use url::Url;
use uuid::Uuid;

pub type AuthResult<T> = Result<T, &'static str>;

pub struct BrowserLogin {
    pub redirect: String,
    pub verifier: String,
    pub challenge: String,
    listener: TcpListener,
    path: String,
}

impl BrowserLogin {
    pub fn bind(port: u16, path: String) -> AuthResult<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .map_err(|_| "The local sign-in port is busy. Close other OPON windows and retry.")?;
        listener
            .set_nonblocking(true)
            .map_err(|_| "Could not start local sign-in.")?;
        let port = listener
            .local_addr()
            .map_err(|_| "Could not read the local sign-in port.")?
            .port();
        let verifier = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        Ok(Self {
            redirect: format!("http://127.0.0.1:{port}{path}"),
            challenge: URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
            verifier,
            listener,
            path,
        })
    }

    pub fn receive(&self, authorize: &Url, state: Option<&str>) -> AuthResult<String> {
        webbrowser::open(authorize.as_str()).map_err(|_| "Could not open your browser.")?;
        let deadline = Instant::now() + Duration::from_secs(150);
        while Instant::now() < deadline {
            let mut stream = match self.listener.accept() {
                Ok((stream, _)) => stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(100));
                    continue;
                }
                Err(_) => return Err("Could not receive the sign-in callback."),
            };
            let timeout = Some(Duration::from_secs(2));
            if stream.set_read_timeout(timeout).is_err()
                || stream.set_write_timeout(timeout).is_err()
            {
                continue;
            }
            let mut request = String::new();
            if BufReader::new((&mut stream).take(8192))
                .read_line(&mut request)
                .is_err()
            {
                continue;
            }
            let mut parts = request.split_whitespace();
            let target = parts
                .next()
                .filter(|method| *method == "GET")
                .and_then(|_| parts.next());
            let url =
                target.and_then(|target| Url::parse(&format!("http://127.0.0.1{target}")).ok());
            let result = url
                .as_ref()
                .and_then(|url| callback(url, &self.path, state));
            let (status, body) = match &result {
                Some(Ok(_)) => ("200 OK", "Sign-in received. Return to OPON."),
                Some(Err(_)) => (
                    "200 OK",
                    "Sign-in was cancelled or refused. Return to OPON.",
                ),
                None => ("404 Not Found", "No matching OPON sign-in."),
            };
            let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            if let Some(result) = result {
                return result;
            }
        }
        Err("Sign-in expired. Try again from Settings.")
    }
}

fn callback(url: &Url, path: &str, state: Option<&str>) -> Option<AuthResult<String>> {
    if url.path() != path
        || state.is_some_and(|state| {
            !url.query_pairs()
                .any(|(key, value)| key == "state" && value == state)
        })
    {
        return None;
    }
    if let Some((_, code)) = url
        .query_pairs()
        .find(|(key, value)| key == "code" && !value.is_empty())
    {
        return Some(Ok(code.into_owned()));
    }
    url.query_pairs()
        .any(|(key, _)| key == "error")
        .then_some(Err("Sign-in was cancelled or refused."))
}
