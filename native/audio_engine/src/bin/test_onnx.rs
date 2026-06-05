fn main() {
    println!("Testing ONNX runtime...");
    let ort_dylib = std::env::var("ORT_DYLIB_PATH")
        .unwrap_or_else(|_| "./onnxruntime.dll".to_string());
    println!("ORT_DYLIB_PATH: {}", ort_dylib);
    
    println!("Environment variables:");
    for (key, value) in std::env::vars() {
        if key.starts_with("TRANSLATOR_") || key.contains("DEEPGRAM") || key.contains("GROQ") {
            println!("  {} = {}", key, if value.len() > 20 { format!("{}...", &value[..20]) } else { value });
        }
    }
    
    println!("\nTest complete!");
}
