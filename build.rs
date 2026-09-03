fn main() -> Result<(), Box<dyn std::error::Error>> {
    let git_sha = std::process::Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|sha| sha.trim().to_string())
        .filter(|sha| !sha.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=CIPHERSCAN_GIT_SHA={git_sha}");
    println!("cargo:rerun-if-changed=.git/HEAD");

    tonic_build::compile_protos("proto/indexer.proto")?;
    Ok(())
}
