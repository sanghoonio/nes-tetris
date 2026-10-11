//! Rooms served on this machine, for trying matches without Cloudflare:
//!
//!     cargo run --release -p netplay --bin local-room [port]
//!
//! then clients with `--server ws://localhost:8788` (or the port given).

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let port = std::env::args().nth(1).unwrap_or_else(|| "8788".to_string());
    let listener = std::net::TcpListener::bind(format!("0.0.0.0:{port}")).unwrap_or_else(|e| panic!("port {port}: {e}"));
    println!("rooms on ws://localhost:{port}");
    netplay::local::serve(listener);
}

#[cfg(target_arch = "wasm32")]
fn main() {}
