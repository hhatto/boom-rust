extern crate boom;
extern crate getopts;
extern crate hyper;
extern crate url;
extern crate time;
extern crate mime;
use getopts::Options;
use mime::Mime;
use hyper::Client;
use hyper::client::Body;
use hyper::method::Method;
use hyper::status::StatusCode;
use hyper::header::{UserAgent, Connection, AcceptEncoding, Encoding, qitem, Headers, ContentType, Authorization, Basic};
use std::str::FromStr;
use std::{env, thread};
use std::time::Duration;
use std::process;
use std::io::Cursor;
use std::io::prelude::*;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Receiver};

const N_DEFAULT: i32 = 200;
const C_DEFAULT: i32 = 50;

mod report;
use report::Report;

#[derive(Clone)]
struct BoomOption {
    concurrency: i32,
    num_requests: i32,
    method: Method,
    url: String,
    body: String,
    username: String,
    password: String,
    proxy_host: String,
    proxy_port: u16,
    keepalive: bool,
    compress: bool,
    mime: Mime,
}

struct WorkerOption {
    opts: BoomOption,
    report: Arc<Mutex<Report>>,
}

fn get_request(options: &BoomOption) -> Client {
    let mut client = if options.proxy_host.is_empty() {
        Client::new()
    } else {
        Client::with_http_proxy(options.proxy_host.to_owned(), options.proxy_port)
    };
    let timeout: Option<Duration> = Some(Duration::new(1, 0));
    client.set_read_timeout(timeout);
    return client;
}

// one request
fn b(client: &Arc<Client>, options: BoomOption, report: Arc<Mutex<Report>>) -> bool {
    let request_body = options.body.clone();
    let mut cursor: Cursor<&[u8]> = Cursor::new(request_body.as_bytes());
    let mut headers = Headers::new();
    let mut req = client.request(options.method, options.url.as_str()).header(UserAgent("boom-rust".to_string()));
    if !options.keepalive {
        req = req.header(Connection::close());
    }
    if !options.body.is_empty() {
        req = req.body(Body::SizedBody(&mut cursor, options.body.len() as u64));
    }
    headers.set(ContentType(options.mime));
    if options.compress {
        headers.set(AcceptEncoding(vec![qitem(Encoding::Gzip)]));
    }
    if !options.username.is_empty() {
        headers.set(Authorization(Basic {
            username: options.username,
            password: Some(options.password),
        }));
    }

    req = req.headers(headers);

    let t1 = time::now();
    let mut res = req.send().unwrap();
    let t2 = time::now();
    let diff = (t2 - t1).num_microseconds().unwrap() as f32;

    {
        let mut r = report.lock().unwrap();
        let millisec = diff / 1000.;
        (*r).time_total += millisec;
        (*r).req_num += 1;
        (*r).results.push((res.status.to_u16(), millisec));
    }

    if res.status != StatusCode::Ok {
        let mut r = report.lock().unwrap();
        let mut status_num = (*r).status_num.entry(res.status.to_u16()).or_insert(0);
        *status_num += 1;
        return false;
    }

    let mut body = vec![0 as u8; 0];
    let content_len = res.read_to_end(&mut body).unwrap();
    {
        let mut r = report.lock().unwrap();
        (*r).size_total += content_len as i64;
    }

    let mut r = report.lock().unwrap();
    let mut status_num = (*r).status_num.entry(200).or_insert(0);
    *status_num += 1;
    return true;
}

// exec actions
fn exec_boom(client: &Arc<Client>, options: BoomOption, report: Arc<Mutex<Report>>) {
    Some(b(client, options, report));
}

fn exec_worker(client: &Arc<Client>, rx: Receiver<Option<WorkerOption>>) {
    loop {
        match rx.recv().unwrap() {
            Some(wconf) => {
                exec_boom(client, wconf.opts, wconf.report);
            }
            None => {
                break;
            }
        }
    }
}

fn print_usage(opts: Options) {
    print!("{}", opts.usage("Usage: boom-rust [options] URL"));
    print!("{}", opts.short_usage("boom-rust"));
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut opts = Options::new();
    opts.optopt("n", "num", "number of requests", "N");
    opts.optopt("c", "concurrency", "concurrency", "C");
    opts.optopt("m", "method", "HTTP method (GET, POST, PUT, DELETE, HEAD, OPTIONS)", "METHOD");
    opts.optopt("d", "data", "HTTP request body data", "DATA");
    opts.optopt("T", "", "Content-type, defaults to \"text/html\".", "ContentType");
    opts.optopt("a", "", "use basic authentication", "USERNAME:PASSWORD");
    opts.optopt("x", "", "HTTP proxy address as host:port", "PROXY_HOST:PROXY_PORT");
    opts.optflag("", "disable-compress", "Disable compress");
    opts.optflag("", "disable-keepalive", "Disable keep-alive");
    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(_) => {
            print_usage(opts);
            return;
        }
    };
    if matches.free.len() < 1 {
        print_usage(opts);
        return;
    }

    let mime_v = match matches.opt_str("T") {
        Some(v) => v,
        None => "text/html".to_string(),
    };
    let method_v = match matches.opt_str("m") {
        Some(v) => v.to_uppercase(),
        None => "GET".to_string(),
    };
    let body_v = match matches.opt_str("d") {
        Some(v) => v.to_string(),
        None => "".to_string(),
    };
    let (basic_auth_name, basic_auth_pass) = match matches.opt_str("a") {
        Some(v) => {
            let s: Vec<&str> = v.split(':').collect();
            let ret: (String, String) = if s.len() != 2 {
                println!("invalid argument: {}\n", v);
                print_usage(opts);
                process::exit(1);
            } else {
                (s[0].to_string(), s[1].to_string())
            };
            ret
        }
        None => ("".to_string(), "".to_string()),
    };
    let (proxy_host, proxy_port) = match matches.opt_str("x") {
        Some(v) => {
            let s: Vec<&str> = v.split(':').collect();
            let ret: (String, u16) = if s.len() != 2 {
                println!("invalid argument: {}\n", v);
                print_usage(opts);
                process::exit(1);
            } else {
                match u16::from_str_radix(s[1], 10) {
                    Ok(v) => (s[0].to_string(), v),
                    Err(_) => {
                        println!("invalid proxy address: {}\n", v);
                        print_usage(opts);
                        process::exit(1);
                    }
                }
            };
            ret
        }
        None => ("".to_string(), 0),
    };
    let mut opt = BoomOption {
        concurrency: 0,
        num_requests: 0,
        method: Method::from_str(method_v.as_str()).unwrap(),
        url: matches.free[0].clone(),
        body: body_v,
        username: basic_auth_name,
        password: basic_auth_pass,
        proxy_host: proxy_host,
        proxy_port: proxy_port,
        mime: Mime::from_str(mime_v.as_str()).unwrap(),
        keepalive: !matches.opt_present("disable-keepalive"),
        compress: !matches.opt_present("disable-compress"),
    };
    opt.concurrency = match matches.opt_str("c") {
        Some(v) => i32::from_str_radix(&v, 10).unwrap(),
        None => C_DEFAULT,
    };
    opt.num_requests = match matches.opt_str("n") {
        Some(v) => i32::from_str_radix(&v, 10).unwrap(),
        None => N_DEFAULT,
    };
    if matches.free.is_empty() {
        print_usage(opts);
        return;
    };

    let mut handles = vec![];
    let mut workers = vec![];

    let client = Arc::new(get_request(&opt));

    // create worker
    for _ in 0..opt.concurrency {
        let (worker_tx, worker_rx) = channel::<Option<WorkerOption>>();
        workers.push(worker_tx.clone());
        let c = client.clone();
        handles.push(thread::spawn(move || exec_worker(&c, worker_rx)));
    }

    let t1 = time::now();

    let report = Arc::new(Mutex::new(Report::new()));
    // request for attack
    for cnt in 0..opt.num_requests {
        let w = WorkerOption {
            opts: opt.clone(),
            report: report.clone(),
        };
        let offset = ((cnt as i32) % opt.concurrency) as usize;
        let req = workers[offset].clone();
        req.send(Some(w)).unwrap();
    }

    // exit for worker
    for worker in workers {
        worker.send(None).unwrap();
    }

    for handle in handles {
        handle.join().unwrap();
    }
    let t2 = time::now();
    let diff = (t2 - t1).num_microseconds().unwrap() as f32;

    let request_per_seconds = 1000000. * opt.num_requests as f32 / diff;

    {
        let r = report.clone();
        let mut report_mut = (*r).lock().unwrap();
        report_mut.time_exec_total = diff / 1000.;
        report_mut.req_per_sec = request_per_seconds;
        report_mut.finalize();
    }
}
