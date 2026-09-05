//! Echo load generator.
//!
//! Uses blocking sockets on plain threads, so nothing about the measurement
//! depends on the async runtime under test.
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let mut args = std::env::args().skip(1);
    let addr = args.next().unwrap_or_else(|| "127.0.0.1:8080".into());
    let conns: usize = args.next().and_then(|v| v.parse().ok()).unwrap_or(64);
    let secs: u64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(10);
    let payload: usize = args.next().and_then(|v| v.parse().ok()).unwrap_or(128);

    let stop = Arc::new(AtomicBool::new(false));
    let total = Arc::new(AtomicU64::new(0));
    let mut latencies: Vec<Vec<u64>> = Vec::new();
    let mut handles = Vec::new();

    for _ in 0..conns {
        let addr = addr.clone();
        let stop = stop.clone();
        let total = total.clone();
        handles.push(std::thread::spawn(move || {
            let mut sock = TcpStream::connect(&addr).expect("connect");
            sock.set_nodelay(true).unwrap();
            let out = vec![b'x'; payload];
            let mut buf = vec![0u8; payload];
            let mut lat = Vec::with_capacity(1 << 16);

            while !stop.load(Ordering::Relaxed) {
                let t = Instant::now();
                if sock.write_all(&out).is_err() {
                    break;
                }
                if sock.read_exact(&mut buf).is_err() {
                    break;
                }
                lat.push(t.elapsed().as_nanos() as u64);
                total.fetch_add(1, Ordering::Relaxed);
            }
            lat
        }));
    }

    let start = Instant::now();
    std::thread::sleep(Duration::from_secs(secs));
    stop.store(true, Ordering::Relaxed);
    for h in handles {
        latencies.push(h.join().unwrap());
    }
    let elapsed = start.elapsed().as_secs_f64();

    let mut all: Vec<u64> = latencies.into_iter().flatten().collect();
    all.sort_unstable();
    let n = all.len();
    let pct = |p: f64| -> f64 {
        if n == 0 {
            return 0.0;
        }
        all[((n as f64 * p) as usize).min(n - 1)] as f64 / 1000.0
    };

    let count = total.load(Ordering::Relaxed);
    println!("target      {addr}");
    println!("conns       {conns}");
    println!("payload     {payload} bytes");
    println!("duration    {elapsed:.2} s");
    println!("requests    {count}");
    println!("qps         {:.0}", count as f64 / elapsed);
    println!("p50         {:.1} us", pct(0.50));
    println!("p99         {:.1} us", pct(0.99));
}
