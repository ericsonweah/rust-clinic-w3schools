use std::collections::HashMap;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::thread;

//
// ---------- HttpResponse ----------
//
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: String,
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl HttpResponse {
    pub fn ok(body: &str) -> Self {
        let mut headers = HashMap::new();
        headers.insert("Content-Type".into(), "text/plain".into());
        headers.insert("Content-Length".into(), body.len().to_string());
        Self {
            status: "HTTP/1.1 200 OK".into(),
            headers,
            body: body.into(),
        }
    }

    pub fn with_status(code: u16, body: &str, content_type: &str) -> Self {
        let mut headers = HashMap::new();
        headers.insert("Content-Type".into(), content_type.into());
        headers.insert("Content-Length".into(), body.len().to_string());
        Self {
            status: format!("HTTP/1.1 {} CUSTOM", code),
            headers,
            body: body.into(),
        }
    }

    pub fn to_string(&self) -> String {
        let mut resp = format!("{}\r\n", self.status);
        for (k, v) in &self.headers {
            resp.push_str(&format!("{}: {}\r\n", k, v));
        }
        resp.push_str("\r\n");
        resp.push_str(&self.body);
        resp
    }
}

//
// ---------- res module ----------
//
pub mod res {
    use super::HttpResponse;

    pub fn ok(body: &str) -> HttpResponse {
        HttpResponse::ok(body)
    }

    pub fn not_found(body: &str) -> HttpResponse {
        HttpResponse::with_status(404, body, "text/plain")
    }

    pub fn json(body: &str) -> HttpResponse {
        HttpResponse::with_status(200, body, "application/json")
    }
}

//
// ---------- App ----------
//
pub struct App {
    routes: HashMap<String, Box<dyn Fn(&str) -> HttpResponse + Send + Sync>>,
}

impl App {
    pub fn new() -> Self {
        Self {
            routes: HashMap::new(),
        }
    }

    pub fn get<F>(&mut self, path: &str, handler: F)
    where
        F: Fn(&str) -> HttpResponse + Send + Sync + 'static,
    {
        self.routes.insert(format!("GET {}", path), Box::new(handler));
    }

    pub fn listen(self, addr: &str) -> std::io::Result<()> {
        let listener = TcpListener::bind(addr)?;
        println!("Server running at http://{}", addr);

        for stream in listener.incoming() {
            let stream = stream?;
            let routes = self.routes.clone();
            thread::spawn(move || {
                handle_connection(stream, routes);
            });
        }
        Ok(())
    }
}

//
// ---------- Connection Handler ----------
//
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, Box<dyn Fn(&str) -> HttpResponse + Send + Sync>>) {
    let mut buffer = [0; 512];
    let _ = stream.read(&mut buffer);

    let req_str = String::from_utf8_lossy(&buffer);
    let mut lines = req_str.lines();
    let request_line = lines.next().unwrap_or("");
    let parts: Vec<&str> = request_line.split_whitespace().collect();

    let response = if parts.len() >= 2 {
        let key = format!("{} {}", parts[0], parts[1]);
        if let Some(handler) = routes.get(&key) {
            handler(parts[1])
        } else {
            res::not_found("404 Not Found")
        }
    } else {
        res::not_found("400 Bad Request")
    };

    let resp_str = response.to_string();
    let _ = stream.write_all(resp_str.as_bytes());
    let _ = stream.flush();
}

//
// ---------- main ----------
//
fn main() {
    let mut app = App::new();

    app.get("/", |_req| {
        res::ok("Hello RawExpress!")
    });

    app.listen("127.0.0.1:3000").unwrap();
}
