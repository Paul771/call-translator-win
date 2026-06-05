use std::time::Instant;
use ort;

fn main() {
    println!("=== test_tts ===");
    println!("CWD: {:?}", std::env::current_dir().unwrap());
    println!("ORT_DYLIB_PATH: {:?}", std::env::var("ORT_DYLIB_PATH"));

    let project = std::env::current_dir().unwrap()
        .ancestors().skip_while(|p| !p.join("native").exists())
        .next().unwrap_or(std::path::Path::new("."))
        .to_path_buf();

    println!("Project: {:?}", project);

    let model_path = project.join("models").join("piper-en").join("en_GB-alan-low.onnx");
    let config_path = project.join("models").join("piper-en").join("en_GB-alan-low.onnx.json");

    println!("Model: {:?} (exists: {})", model_path, model_path.exists());
    println!("Config: {:?} (exists: {})", config_path, config_path.exists());

    // Step 1: ort::init()
    let t0 = Instant::now();
    let ok = ort::init().commit();
    println!("ort::init().commit() => {ok} in {:?}", t0.elapsed());

    // Step 2: Session::builder()
    let t1 = Instant::now();
    let mut builder = match ort::session::Session::builder() {
        Ok(b) => { println!("Session::builder() OK in {:?}", t1.elapsed()); b }
        Err(e) => { println!("Session::builder() FAIL: {e}"); return; }
    };

    // Step 3: commit_from_file
    let t2 = Instant::now();
    match builder.commit_from_file(model_path.to_str().unwrap()) {
        Ok(session) => {
            println!("commit_from_file() OK in {:?}", t2.elapsed());
            println!("Session created successfully!");
            // Try a simple inference
            let _ = session;
        }
        Err(e) => {
            println!("commit_from_file() FAIL: {e}");
        }
    }
}
