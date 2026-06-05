fn main() {
    println!("DEEPGRAM_API_KEY: {:?}", std::env::var("DEEPGRAM_API_KEY").unwrap_or_else(|_| "NOT SET".to_string()));
    println!("GROQ_API_KEY: {:?}", std::env::var("GROQ_API_KEY").unwrap_or_else(|_| "NOT SET".to_string()));
    println!("TRANSLATOR_MIC_DEVICE: {:?}", std::env::var("TRANSLATOR_MIC_DEVICE").unwrap_or_else(|_| "NOT SET".to_string()));
}
