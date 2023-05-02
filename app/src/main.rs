use ambient_std::{
    asset_cache::{AssetCache, SyncAssetKeyExt},
    download_asset::AssetsCacheOnDisk, asset_url::{AbsAssetUrl, ContentBaseUrlKey},
};
use clap::Parser;

mod cli;
mod client;
mod server;
mod shared;

use ambient_physics::physx::PhysicsKey;
use anyhow::Context;
use cli::Cli;
use log::LevelFilter;
use server::QUIC_INTERFACE_PORT;

fn setup_logging() -> anyhow::Result<()> {
    const MODULES: &[(LevelFilter, &[&str])] = &[
        (
            LevelFilter::Error,
            &[
                // Warns about extra syntactic elements; we are not concerned with these.
                "fbxcel",
            ],
        ),
        (
            LevelFilter::Warn,
            &[
                "ambient_build",
                "ambient_gpu",
                "ambient_model",
                "ambient_physics",
                "ambient_std",
                "cranelift_codegen",
                "naga",
                "tracing",
                "symphonia_core",
                "symphonia_bundle_mp3",
                "wgpu_core",
                "wgpu_hal",
            ],
        ),
    ];

    // Initialize the logger and lower the log level for modules we don't need to hear from by default.
    #[cfg(not(feature = "tracing"))]
    {
        let mut builder = env_logger::builder();
        builder.filter_level(LevelFilter::Info);

        for (level, modules) in MODULES {
            for module in *modules {
                builder.filter_module(module, *level);
            }
        }

        builder.parse_default_env().try_init()?;

        Ok(())
    }

    #[cfg(feature = "tracing")]
    {
        use tracing::metadata::Level;
        use tracing_log::AsTrace;
        use tracing_subscriber::prelude::*;
        use tracing_subscriber::{registry, EnvFilter};

        let mut filter = tracing_subscriber::filter::Targets::new().with_default(tracing::metadata::LevelFilter::DEBUG);
        for (level, modules) in MODULES {
            for &module in *modules {
                filter = filter.with_target(module, level.as_trace());
            }
        }

        // BLOCKING: pending https://github.com/tokio-rs/tracing/issues/2507
        // let modules: Vec<_> = MODULES.iter().flat_map(|&(level, modules)| modules.iter().map(move |&v| format!("{v}={level}"))).collect();

        // eprintln!("{modules:#?}");
        // let mut filter = tracing_subscriber::filter::EnvFilter::builder().with_default_directive(Level::INFO.into()).from_env_lossy();

        // for module in modules {
        //     filter = filter.add_directive(module.parse().unwrap());
        // }

        // let mut filter = std::env::var("RUST_LOG").unwrap_or_default().parse::<tracing_subscriber::filter::Targets>().unwrap_or_default();
        // filter.extend(MODULES.iter().flat_map(|&(level, modules)| modules.iter().map(move |&v| (v, level.as_trace()))));

        let env_filter = EnvFilter::builder().with_default_directive(Level::INFO.into()).from_env_lossy();

        registry()
            .with(filter)
            .with(env_filter)
            //
            .with(
                tracing_tree::HierarchicalLayer::new(4)
                    .with_indent_lines(true)
                    .with_verbose_entry(true)
                    .with_verbose_exit(true)
                    .with_timer(tracing_tree::time::OffsetDateTime),
                // .with_timer(tracing_tree::time::Uptime::from(std::time::Instant::now())),
            )
            // .with(tracing_subscriber::fmt::Layer::new().pretty())
            .try_init()?;

        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    setup_logging()?;

    shared::components::init()?;
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let assets = AssetCache::new(runtime.handle().clone());
    PhysicsKey.get(&assets); // Load physics
    AssetsCacheOnDisk.insert(&assets, false); // Disable disk caching for now; see https://github.com/AmbientRun/Ambient/issues/81

    let cli = Cli::parse();

    let project_path = cli.project().and_then(|p| p.path.clone());
    let (project_fs_path, project_path) = match project_path {
        Some(project_path) if project_path.starts_with("http://") || project_path.starts_with("https://") => {
            let url = AbsAssetUrl::parse(project_path)?;
            // project path is a URL, so let's use it as the content base URL
            ContentBaseUrlKey.insert(&assets, url.push("build/")?);
            (None, url)
        }
        Some(project_path) => {
            let project_path = std::path::PathBuf::from(project_path);
            let current_dir = std::env::current_dir()?;
            let project_path = if project_path.is_absolute() { project_path } else { ambient_std::path::normalize(&current_dir.join(project_path)) };

            if project_path.exists() && !project_path.is_dir() {
                anyhow::bail!("Project path {project_path:?} exists and is not a directory.");
            }
            let url = AbsAssetUrl::from_directory_path(project_path);
            (url.to_file_path().ok().flatten(), url)
        }
        None => {
            let url = AbsAssetUrl::from_directory_path(std::env::current_dir()?);
            (url.to_file_path().ok().flatten(), url)
        }
    };

    // If new: create project, immediately exit
    if let Cli::New { name, .. } = &cli {
        if let Some(path) = project_fs_path {
            if let Err(err) = cli::new_project::new_project(&path, name.as_deref()) {
                eprintln!("Failed to create project: {err:?}");
            }
        } else {
            eprintln!("Cannot create project in a remote directory.");
        }
        return Ok(());
    }

    // If a project was specified, assume that assets need to be built
    let manifest = cli
        .project()
        .map(|_| {
            if let Some(path) = &project_fs_path {
                // load manifest from file
                anyhow::Ok(ambient_project::Manifest::from_file(path.join("ambient.toml")).context("Failed to read ambient.toml.")?)
            } else {
                // project_path is a URL, so download the pre-build manifest (with resolved imports)
                let manifest_url = project_path.push("build/ambient.toml").unwrap();
                let manifest_data = runtime.block_on(manifest_url.download_string(&assets)).context("Failed to download ambient.toml.")?;
                anyhow::Ok(ambient_project::Manifest::parse(&manifest_data).context("Failed to parse downloaded ambient.toml.")?)
            }
        })
        .transpose()?;

    let metadata = if let Some(manifest) = manifest.as_ref() {
        if !cli.project().unwrap().no_build && project_fs_path.is_some() {
            let project_name = manifest.project.name.as_deref().unwrap_or("project");
            log::info!("Building {}", project_name);
            let metadata = runtime.block_on(ambient_build::build(
                PhysicsKey.get(&assets),
                &assets,
                project_fs_path.clone().unwrap(),
                manifest,
                cli.project().map(|p| p.release).unwrap_or(false),
            ));
            log::info!("Done building {}", project_name);
            Some(metadata)
        } else {
            let metadata_url = project_path.push("build/metadata.toml").unwrap();
            let metadata_data = runtime.block_on(metadata_url.download_string(&assets)).context("Failed to download build/metadata.toml.")?;
            Some(ambient_build::Metadata::parse(&metadata_data)?)
        }
    } else {
        None
    };

    // If this is just a build, exit now
    if matches!(&cli, Cli::Build { .. }) {
        return Ok(());
    }

    // Otherwise, either connect to a server or host one
    let server_addr = if let Cli::Join { host, .. } = &cli {
        if let Some(mut host) = host.clone() {
            if !host.contains(':') {
                host = format!("{host}:{QUIC_INTERFACE_PORT}");
            }
            runtime.block_on(tokio::net::lookup_host(&host))?.next().ok_or_else(|| anyhow::anyhow!("No address found for host {host}"))?
        } else {
            format!("127.0.0.1:{QUIC_INTERFACE_PORT}").parse()?
        }
    } else {
        let port = server::start(&runtime, assets.clone(), cli.clone(), project_path, manifest.as_ref().expect("no manifest"), metadata.as_ref().expect("no build metadata"));
        format!("127.0.0.1:{port}").parse()?
    };

    // Time to join!
    let handle = runtime.handle().clone();
    if let Some(run) = cli.run() {
        // If we have run parameters, start a client and join a server
        runtime.block_on(client::run(assets, server_addr, run, project_fs_path));
    } else {
        // Otherwise, wait for the Ctrl+C signal
        handle.block_on(async move {
            match tokio::signal::ctrl_c().await {
                Ok(()) => {}
                Err(err) => log::error!("Unable to listen for shutdown signal: {}", err),
            }
        });
    }
    Ok(())
}
