use trombone::backend::AudioBackend;
use trombone::backend::android::{AndroidBackend, AndroidBackendKind};
use trombone::core::config::StreamConfig;

fn main() {
    let config = StreamConfig::default();
    let backend = AndroidBackend::new(AndroidBackendKind::AAudio);

    match backend.create_stream(config) {
        Ok(stream) => {
            println!(
                "Created stream: {:?} @ {} Hz",
                stream.config().direction,
                stream.config().sample_rate_hz
            );
        }
        Err(error) => {
            println!("Backend not ready yet: {error:?}");
        }
    }
}
