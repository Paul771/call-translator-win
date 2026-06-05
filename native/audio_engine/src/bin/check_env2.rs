fn main() {
    println!("=== ENV ===");
    for (key, value) in std::env::vars() {
        if key.starts_with("TRANSLATOR_") || key.contains("DEEPGRAM") || key.contains("GROQ") || key.contains("ORT") || key.contains("MODEL") {
            println!("  {} = {}", key, if value.len() > 80 { format!("{}...", &value[..80]) } else { value });
        }
    }
    println!("===========");
    
    // Check if onnxruntime.dll exists at the path
    let ort_path = std::env::var("ORT_DYLIB_PATH").unwrap_or_else(|_| "NOT SET".into());
    println!("ORT_DYLIB_PATH: {}", ort_path);
    println!("ORT_DYLIB exists: {}", std::path::Path::new(&ort_path).exists());
    
    // Check models path
    let models_base = std::env::var("TRANSLATOR_MODELS_DIR").unwrap_or_else(|_| "./models".into());
    println!("Models base: {}", models_base);
    println!("Models dir exists: {}", std::path::Path::new(&models_base).exists());
}
