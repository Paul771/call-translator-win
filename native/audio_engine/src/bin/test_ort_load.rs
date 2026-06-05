use std::time::Instant;

fn main() {
    let start = Instant::now();
    println!("[TEST] Starting ONNX Runtime test...");

    // Step 1: Check DLL exists
    let dll_path = std::env::var("ORT_DYLIB_PATH")
        .unwrap_or_else(|_| "./onnxruntime.dll".to_string());
    println!("[TEST] ONNX Runtime DLL: {}", dll_path);
    if std::path::Path::new(&dll_path).exists() {
        let meta = std::fs::metadata(&dll_path).unwrap();
        println!("[TEST] DLL exists, size: {} bytes", meta.len());
    } else {
        println!("[TEST] DLL NOT FOUND at {}", dll_path);
    }

    // Step 2: Try to create Session builder
    println!("[TEST] Creating Session::builder()...");
    let builder_result = std::panic::catch_unwind(|| {
        ort::session::Session::builder()
    });
    match builder_result {
        Ok(Ok(_builder)) => {
            println!("[TEST] Session::builder() OK in {}ms", start.elapsed().as_millis());
        }
        Ok(Err(e)) => {
            println!("[TEST] Session::builder() FAILED: {} in {}ms", e, start.elapsed().as_millis());
        }
        Err(_) => {
            println!("[TEST] Session::builder() PANICKED after {}ms", start.elapsed().as_millis());
        }
    }

    println!("[TEST] Total time: {}ms", start.elapsed().as_millis());
}
