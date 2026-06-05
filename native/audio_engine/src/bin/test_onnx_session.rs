use std::time::Instant;

fn main() {
    let start = Instant::now();
    println!("test_onnx_session starting (ORT_DYLIB_PATH={:?})", std::env::var("ORT_DYLIB_PATH"));
    
    println!("Step 1: ort::init().commit()...");
    let ok = ort::init().commit();
    println!("Step 1: done in {:?}, ok={}", start.elapsed(), ok);
    
    println!("Step 2: Session::builder()...");
    let builder_start = Instant::now();
    let builder = ort::session::Session::builder();
    println!("Step 2: done in {:?}, ok={:?}", builder_start.elapsed(), builder.is_ok());
    
    if let Ok(_builder) = builder {
        // Don't try loading a model - just test session builder
    }
    
    println!("Total: {:?}", start.elapsed());
}
