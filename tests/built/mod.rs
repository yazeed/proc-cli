//! Where cargo put the example programs the tests run (`examples/listener.rs`,
//! `examples/session.rs`)

use std::path::PathBuf;

/// `target/<profile>/examples/<name>[.exe]`, found by looking up from this
/// test binary. Its own folder depends on the cargo version: Rust 1.99 put it
/// in `target/debug/deps`, 1.100 in `target/debug/build/proc-cli/<hash>/out`,
/// while the examples stay in `target/debug/examples` in both
pub fn example(name: &str) -> PathBuf {
    let me = std::env::current_exe().unwrap();
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    me.ancestors()
        .map(|dir| dir.join("examples").join(&file))
        .find(|path| path.exists())
        .unwrap_or_else(|| {
            panic!(
                "build the example first (cargo build --examples): no examples/{file} above {}",
                me.display()
            )
        })
}
