use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
};

use anyhow::Context;
use clap::{Args, Subcommand, ValueEnum};
use tokio::{join, process::Command, try_join};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub(crate) enum Target {
    /// Generates a wasm and js shim that uses `require` to import the `.wasm`
    Bundler,
    /// The shim won't import the `.wasm` itself, allowing for external fetching
    Standalone,
}

#[derive(Debug, Args, Clone)]
pub struct BuildOptions {
    #[arg(long, default_value = "dev")]
    pub profile: String,
    #[arg(long, value_enum, default_value = "bundler")]
    target: Target,
}

pub async fn run(opts: BuildOptions) -> anyhow::Result<()> {
    ensure_wasm_pack().await?;

    let output_path = run_cargo_build(&opts).await?;

    eprintln!("Built package: {:?}", output_path);

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub(crate) async fn install_wasm_pack() -> anyhow::Result<()> {
    eprintln!("Installing wasm-pack from source");
    let status = Command::new("cargo")
        .args(["install", "wasm-pack"])
        .spawn()?
        .wait()
        .await?;

    if !status.success() {
        anyhow::bail!("Failed to install wasm-pack");
    }

    Ok(())
}

#[cfg(target_os = "linux")]
pub(crate) async fn install_wasm_pack() -> anyhow::Result<()> {
    eprintln!("Installing wasm-pack");
    let curl = Command::new("curl")
        .args([
            "https://rustwasm.github.io/wasm-pack/installer/init.sh",
            "-sSf",
        ])
        .spawn()
        .context("Failed to spawn curl")?;

    if !status.success() {
        anyhow::bail!("Failed to install wasm-pack");
    }

    let sh = Command::new("sh")
        .stdin(Stdio::from(curl.stdout.unwrap()))
        .spawn()
        .context("Failed to spawn sh")?;

    let (curl, sh) = try_join!(curl.wait(), sh.wait())?;

    if !curl.success() {
        anyhow::bail!("Failed to fetch install script")
    }

    if !sh.success() {
        anyhow::bail!("Failed to run install script for wasm-pack")
    }

    Ok(())
}

pub async fn ensure_wasm_pack() -> anyhow::Result<()> {
    match which::which("wasm-pack") {
        Err(_) => {
            install_wasm_pack().await?;
            assert!(which::which("wasm-pack").is_ok(), "wasm-pack is in PATH");

            Ok(())
        }
        Ok(path) => {
            eprintln!("Found installation of wasm pack at {path:?}");
            Ok(())
        }
    }
}

pub async fn run_cargo_build(opts: &BuildOptions) -> anyhow::Result<PathBuf> {
    let mut command = Command::new("wasm-pack");

    command.args(["build", "client"]).current_dir("web");

    match &opts.profile[..] {
        "dev" | "debug" => command.arg("--dev"),
        "release" => command.arg("--release"),
        v => anyhow::bail!("Unknown profile: {v:?}"),
    };

    match opts.target {
        Target::Bundler => command.args(["--target", "bundler"]),
        Target::Standalone => command.args(["--target", "web", "--no-pack"]),
    };

    let mut output_path = ["web"]
        .iter()
        .collect::<PathBuf>()
        .canonicalize()
        .context("Produced build artifact does not exist")?;

    output_path.push("pkg");

    command.arg("--out-dir").arg(output_path.clone());

    eprintln!("Building web client");

    let res = command.spawn()?.wait().await?;
    if !res.success() {
        anyhow::bail!("Building package failed with status code: {res}");
    }

    assert!(output_path.exists());

    Ok(output_path)
}
