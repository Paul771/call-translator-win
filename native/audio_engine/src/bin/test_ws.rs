/// Test WebSocket connectivity to Deepgram.
/// Usage: cargo run --bin test_ws
use std::io::Write;
use tungstenite::client::IntoClientRequest;

fn main() {
    let test_urls = vec![
        "wss://echo.websocket.events/.ws",
        "wss://api.deepgram.com/v1/listen?model=nova-3&language=ru&encoding=linear16&sample_rate=16000&channels=1&interim_results=true&endpointing=300",
    ];

    let dg_key = "98e89c6ff9086af1b84e9d10b297aa5764390576";

    for url_str in test_urls {
        print!("\n🔌 Testing: {} ... ", url_str);
        let _ = std::io::stdout().flush();

        let mut request = match url_str.into_client_request() {
            Ok(r) => r,
            Err(e) => {
                println!("❌ Request build failed: {:?}", e);
                continue;
            }
        };

        if url_str.contains("deepgram") {
            request.headers_mut().insert(
                "Authorization",
                format!("Token {}", dg_key).parse().unwrap(),
            );
        }

        match tungstenite::connect(request) {
            Ok((mut ws, resp)) => {
                println!("✅ CONNECTED (HTTP {})", resp.status());
                println!("   Response headers:");
                for (k, v) in resp.headers() {
                    println!("   {}: {:?}", k, v);
                }

                // Send a binary frame
                let test_data = vec![0u8; 16000]; // 1 second of silence at 16kHz
                match ws.send(tungstenite::Message::Binary(test_data)) {
                    Ok(_) => println!("   ✅ Audio sent"),
                    Err(e) => println!("   ❌ Send failed: {:?}", e),
                }

                // Wait a moment and try to read
                std::thread::sleep(std::time::Duration::from_millis(500));
                match ws.read() {
                    Ok(msg) => println!("   ✅ Read: {:?}", msg),
                    Err(tungstenite::Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        println!("   ⏳ WouldBlock (expected, non-blocking)");
                    }
                    Err(e) => println!("   ❌ Read error: {:?}", e),
                }

                let _ = ws.close(None);
                println!("   ✅ Connection closed");
            }
            Err(e) => {
                println!("❌ FAILED: {:?}", e);
                match &e {
                    tungstenite::Error::Http(resp) => {
                        println!("   HTTP response: {}", resp.status());
                        if let Some(body) = resp.body().as_ref() {
                            if let Ok(text) = std::str::from_utf8(body) {
                                println!("   Body: {}", text);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
