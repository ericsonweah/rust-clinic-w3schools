use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use futures::executor::block_on;
use futures::FutureExt; // for .boxed()
use regex::Regex;
use std::future::Future;
use std::pin::Pin;

//
// ---------- BoxFuture alias ----------
//
//pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

type Next<'a> = &'a dyn Fn(HttpRequest) -> BoxFuture<HttpResponse>;
type Middleware = Box<dyn Fn(HttpRequest, Next) -> BoxFuture<HttpResponse> + Send + Sync>;
type Handler = Box<dyn Fn(HttpRequest) -> BoxFuture<HttpResponse> + Send + Sync>;


//
// ---------- HttpRequest ----------
//
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub params: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body_raw: String,
    pub body: HashMap<String, String>, // parsed (form/json simplified)
}

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

    // ---------- Builder-style methods ----------
    pub fn set_header(mut self, key: &str, value: &str) -> Self {
        self.headers.insert(key.to_string(), value.to_string());
        self
    }

    pub fn status(mut self, code: u16, text: &str) -> Self {
        self.status = format!("HTTP/1.1 {} {}", code, text);
        self
    }

    pub fn body(mut self, body: &str) -> Self {
        self.headers
            .insert("Content-Length".into(), body.len().to_string());
        self.body = body.to_string();
        self
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
// ---------- Route matcher ----------
//
fn match_route(pattern: &str, path: &str) -> Option<HashMap<String, String>> {
    let pat_parts: Vec<&str> = pattern.trim_matches('/').split('/').collect();
    let path_parts: Vec<&str> = path.trim_matches('/').split('/').collect();

    let mut params = HashMap::new();

    let mut i = 0;
    while i < pat_parts.len() {
        let p = pat_parts[i];
        let a = path_parts.get(i).unwrap_or(&"");

        if p == "*" {
            let rest = path_parts[i..].join("/");
            params.insert("wildcard".to_string(), rest);
            return Some(params);
        } else if p.starts_with(':') {
            if let Some(open) = p.find('(') {
                let name = &p[1..open];
                let regex_pat = &p[open..];
                let re = Regex::new(regex_pat).ok()?;
                if re.is_match(a) {
                    params.insert(name.to_string(), a.to_string());
                } else {
                    return None;
                }
            } else {
                let name = &p[1..];
                params.insert(name.to_string(), a.to_string());
            }
        } else if p != *a {
            return None;
        }
        i += 1;
    }

    if pat_parts.len() != path_parts.len() && !pat_parts.contains(&"*") {
        return None;
    }

    Some(params)
}

//
// ---------- Middleware types ----------
//
type Next<'a> = &'a dyn Fn(HttpRequest) -> BoxFuture<'static, HttpResponse>;
type Middleware =
    Box<dyn Fn(HttpRequest, Next) -> BoxFuture<'static, HttpResponse> + Send + Sync>;
type Handler = Box<dyn Fn(HttpRequest) -> BoxFuture<'static, HttpResponse> + Send + Sync>;

//
// ---------- App ----------
//
pub struct App {
    routes: Arc<Vec<(String, String, Handler)>>,
    middlewares: Arc<Vec<Middleware>>,
}

impl App {
    pub fn new() -> Self {
        Self {
            routes: Arc::new(Vec::new()),
            middlewares: Arc::new(Vec::new()),
        }
    }

    pub fn get<F, Fut>(&mut self, path: &str, handler: F)
    where
        F: Fn(HttpRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HttpResponse> + Send + 'static,
    {
        Arc::get_mut(&mut self.routes)
            .unwrap()
            .push((
                "GET".into(),
                path.into(),
                Box::new(move |req| handler(req).boxed()),
            ));
    }

    pub fn post<F, Fut>(&mut self, path: &str, handler: F)
    where
        F: Fn(HttpRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HttpResponse> + Send + 'static,
    {
        Arc::get_mut(&mut self.routes)
            .unwrap()
            .push((
                "POST".into(),
                path.into(),
                Box::new(move |req| handler(req).boxed()),
            ));
    }

    pub fn use_middleware<F, Fut>(&mut self, f: F)
    where
        F: Fn(HttpRequest, Next) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HttpResponse> + Send + 'static,
    {
        Arc::get_mut(&mut self.middlewares)
            .unwrap()
            .push(Box::new(move |req, next| f(req, next).boxed()));
    }

    pub fn listen(self, addr: &str) -> std::io::Result<()> {
        let listener = TcpListener::bind(addr)?;
        println!("Server running at http://{}", addr);

        for stream in listener.incoming() {
            let stream = stream?;
            let routes = self.routes.clone();
            let mws = self.middlewares.clone();
            thread::spawn(move || {
                handle_connection(stream, routes, mws);
            });
        }
        Ok(())
    }
}

//
// ---------- Middleware runner ----------
//
fn run_pipeline(
    req: HttpRequest,
    mws: &[Middleware],
    routes: &[(String, String, Handler)],
) -> BoxFuture<HttpResponse> {
    if let Some((first, rest)) = mws.split_first() {
        let next: Next = &|req2: HttpRequest| run_pipeline(req2, rest, routes);
        first(req, next)
    } else {
        async move {
            for (m, pattern, handler) in routes.iter() {
                if &req.method == m {
                    if let Some(params) = match_route(pattern, &req.path) {
                        let mut new_req = req.clone();
                        new_req.params = params;
                        return handler(new_req).await;
                    }
                }
            }
            res::not_found("404 Not Found")
        }
        .boxed()
    }
}

//
// ---------- Request Parser ----------
//
fn parse_request(buffer: &[u8]) -> HttpRequest {
    let req_str = String::from_utf8_lossy(buffer).to_string();
    let mut lines = req_str.split("\r\n");

    let request_line = lines.next().unwrap_or("");
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    let method = parts.get(0).unwrap_or(&"GET").to_string();
    let path = parts.get(1).unwrap_or(&"/").to_string();

    let mut headers = HashMap::new();
    for line in &mut lines {
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(":") {
            headers.insert(k.trim().to_string(), v.trim().to_string());
        }
    }

    let body_raw: String = lines.collect::<Vec<&str>>().join("\n");
    let mut body = HashMap::new();

    if let Some(ct) = headers.get("Content-Type") {
        if ct.contains("application/x-www-form-urlencoded") {
            for pair in body_raw.split('&') {
                if let Some((k, v)) = pair.split_once('=') {
                    body.insert(k.to_string(), v.to_string());
                }
            }
        } else if ct.contains("application/json") {
            let trimmed = body_raw.trim().trim_matches(|c| c == '{' || c == '}');
            for pair in trimmed.split(',') {
                if let Some((k, v)) = pair.split_once(':') {
                    body.insert(
                        k.trim().trim_matches('"').to_string(),
                        v.trim().trim_matches('"').to_string(),
                    );
                }
            }
        }
    }

    HttpRequest {
        method,
        path,
        params: HashMap::new(),
        headers,
        body_raw,
        body,
    }
}

//
// ---------- Connection Handler ----------
//
fn handle_connection(
    mut stream: TcpStream,
    routes: Arc<Vec<(String, String, Handler)>>,
    middlewares: Arc<Vec<Middleware>>,
) {
    let mut buffer = [0; 2048];
    let _ = stream.read(&mut buffer);

    let req = parse_request(&buffer);
    let fut = run_pipeline(req, &middlewares, &routes);
    let response = block_on(fut);

    let resp_str = response.to_string();
    let _ = stream.write_all(resp_str.as_bytes());
    let _ = stream.flush();
}

//
// ---------- main ----------
//
fn main() {
    let mut app = App::new();

    // Async middleware
    app.use_middleware(|req, next| async move {
        println!("[LOG] {} {}", req.method, req.path);
        next(req).await
    });

    // Routes
    app.get("/", |_req| async move {
        res::ok("Welcome")
            .status(200, "OK")
            .set_header("X-Powered-By", "RawExpress")
            .body("Welcome to RawExpress async prototype!")
    });

    app.get("/users/:id", |req| async move {
        res::ok(&format!("Fetched user {}", req.params["id"]))
    });

    app.post("/echo", |req| async move {
        res::ok(&format!("Raw body: {}", req.body_raw))
    });

    app.post("/login", |req| async move {
        let username = req.body.get("username").map(|s| s.as_str()).unwrap_or("");
        let password = req.body.get("password").map(|s| s.as_str()).unwrap_or("");
        res::ok(&format!("Login -> user: {}, pass: {}", username, password))
    });

    app.post("/api", |req| async move {
        let msg = req.body.get("msg").map(|s| s.as_str()).unwrap_or("");
        res::json(&format!(r#"{{"echo":"{}"}}"#, msg))
    });

    app.listen("127.0.0.1:3000").unwrap();
}
