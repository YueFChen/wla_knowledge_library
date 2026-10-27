mod workbench;
fn main() {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init();
    if let Err(error) = workbench::run() {
        eprintln!("wla_knowledge_library backend stopped: {error}");
        std::process::exit(1);
    }
}
