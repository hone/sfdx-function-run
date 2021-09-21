use dkregistry::{render, v2::Client};
use futures::future::try_join_all;
use libcnb::data::launch::ProcessType;
use std::{
    fs,
    path::{Path, PathBuf},
    process,
};

const DEFAULT_STACK_ID: &str = "io.buildpacks.stacks.bionic";

async fn download_image(image: &str, reference: &str, path: impl AsRef<Path>) -> PathBuf {
    let host = "public.ecr.aws";
    let login_scope = format!("repository:{}:pull", image);
    let scopes = vec![login_scope.as_str()];
    let client = Client::configure()
        .insecure_registry(false)
        .registry(host)
        .username(None)
        .password(None)
        .build()
        .unwrap()
        .authenticate(scopes.as_slice())
        .await
        .unwrap();

    println!("Fetching manifest for {}", image);

    let manifest = client.get_manifest(image, reference).await.unwrap();
    let layers_digests = manifest.layers_digests(None).unwrap();

    println!("{} -> got {} layer(s)", &image, layers_digests.len());

    let blob_futures = layers_digests
        .iter()
        .map(|layer_digest| client.get_blob(&image, &layer_digest))
        .collect::<Vec<_>>();

    let blobs = try_join_all(blob_futures).await.unwrap();

    println!("Downloaded {} layers", blobs.len());

    std::fs::create_dir(&path).unwrap();
    let can_path = path.as_ref().canonicalize().unwrap();

    println!("Unpacking layers to {:?}", &can_path);
    render::unpack(&blobs, &can_path).unwrap();

    can_path
}

/// Find the default config dir
fn default_config_dir() -> Option<PathBuf> {
    home::home_dir().map(|path| path.join(".sfdx-function-run"))
}

/// Run `bin/detect` for the buildpack
fn detect(buildpack_dir: &Path, home_dir: &Path, platform_dir: &Path, plan: &Path) -> bool {
    let detect = buildpack_dir.join("bin/detect");
    let exit_status = process::Command::new(&detect)
        .arg(platform_dir)
        .arg(plan)
        .env("CNB_BUILDPACK_DIR", buildpack_dir)
        .env("CNB_STACK_ID", DEFAULT_STACK_ID)
        .env("HOME", home_dir)
        .status();

    exit_status.map(|status| status.success()).unwrap_or(false)
}

/// Run `bin/build` for the buildpack
fn build(
    buildpack_dir: &Path,
    home_dir: &Path,
    layers_dir: &Path,
    platform_dir: &Path,
    plan: &Path,
) -> Result<process::Output, std::io::Error> {
    let build = buildpack_dir.join("bin/build");
    process::Command::new(build)
        .arg(layers_dir)
        .arg(platform_dir)
        .arg(plan)
        .env("CNB_BUILDPACK_DIR", buildpack_dir)
        .env("CNB_STACK_ID", DEFAULT_STACK_ID)
        .env("HOME", home_dir)
        .output()
}

#[tokio::main]
async fn main() {
    let config_dir = match default_config_dir() {
        Some(config_dir) => config_dir,
        None => {
            eprintln!("Could not find HOME DIR.");
            process::exit(100);
        }
    };

    let home_dir = config_dir.join("home");
    let platform_dir = config_dir.join("platform");
    // TODO this should be made per app
    let layers_dir = config_dir.join("layers");
    // TODO clean up first
    let tmp_dir = config_dir.join("tmp");
    let plan = config_dir.join("plan.toml");
    let build_plan = config_dir.join("build_plan.toml");
    let buildpacks_dir = config_dir.join("buildpacks");

    for dir in [&platform_dir, &layers_dir, &tmp_dir, &buildpacks_dir] {
        fs::create_dir_all(dir).unwrap();
    }
    for file in [&plan, &build_plan] {
        // touch file
        fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(file)
            .unwrap();
    }

    let buildpack_dir = download_image(
        "heroku-buildpacks/heroku-jvm-function-invoker-buildpack",
        "sha256:a358b25816d03ce210f7aaf068ea0dae34053421b8e857cd5da2809745626c55",
        buildpacks_dir.join("heroku-jvm-function-invoker-buildpack"),
    )
    .await
    .join("cnb")
    .join("buildpacks")
    .join("heroku_jvm-function-invoker")
    .join("0.5.2");

    // TODO need to resolve dependencies still
    if !detect(&buildpack_dir, &home_dir, &platform_dir, &plan) {
        eprintln!("No buildpacks detected");
        process::exit(200);
    }

    // TODO stream stderr/stdout
    let output = build(&buildpack_dir, &home_dir, &layers_dir, &platform_dir, &plan).unwrap();
    println!("{}", String::from_utf8_lossy(&output.stdout));
    if !output.status.success() {
        eprintln!("bin/build did not exit successfully: {}", output.status);
        process::exit(201);
    }

    let launch_toml = layers_dir.join("launch.toml");
    let launch: libcnb::data::launch::Launch =
        toml::from_str(&fs::read_to_string(&launch_toml).unwrap()).unwrap();

    if let Some(process) = launch
        .processes
        .iter()
        .find(|process| process.r#type == "web".parse::<ProcessType>().unwrap())
    {
        let mut child = process::Command::new(&process.command)
            .args(&process.args)
            .stdin(process::Stdio::inherit())
            .stdout(process::Stdio::inherit())
            .spawn()
            .unwrap();

        child.wait().unwrap();
    }
}
