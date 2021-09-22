use dkregistry::{render, v2::Client};
use function_run::buildpack::Buildpack;
use futures::future::try_join_all;
use libcnb::data::launch::ProcessType;
use std::{
    fs,
    path::{Path, PathBuf},
    process,
};

const DEFAULT_STACK_ID: &str = "io.buildpacks.stacks.bionic";

async fn download_image(host: &str, image: &str, reference: &str, path: impl AsRef<Path>) {
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
        .map(|layer_digest| client.get_blob(image, layer_digest))
        .collect::<Vec<_>>();

    let blobs = try_join_all(blob_futures).await.unwrap();

    println!("Downloaded {} layers", blobs.len());

    std::fs::create_dir(&path).unwrap();
    let can_path = path.as_ref().canonicalize().unwrap();

    println!("Unpacking layers to {:?}", &can_path);
    render::unpack(&blobs, &can_path).unwrap();
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

async fn setup_buildpack_dir(buildpack: &Buildpack, buildpacks_dir: impl AsRef<Path>) -> PathBuf {
    let entries = buildpack.fetch().await.unwrap();
    let entry = match entries
        .iter()
        .find(|entry| entry.version == semver::Version::new(0, 5, 2))
    {
        Some(version) => version,
        None => {
            eprintln!("No valid version");
            std::process::exit(1);
        }
    };

    let mut split = entry.address.split('@');
    let mut split2 = split.next().unwrap().splitn(2, '/');
    let host = split2.next().unwrap();
    let image = split2.next().unwrap();
    let reference = split.next().unwrap();
    let buildpack_download_dir = buildpacks_dir
        .as_ref()
        .join(format!("{}-{}", buildpack, entry.version));
    let buildpack_dir = buildpack_download_dir
        .join("cnb")
        .join("buildpacks")
        .join(buildpack.to_string())
        .join(entry.version.to_string());

    if buildpack_download_dir.exists() {
        println!("Using existing {}-{}", buildpack, entry.version);
    } else {
        download_image(host, image, reference, buildpack_download_dir).await;
    }

    buildpack_dir
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

    let buildpack = Buildpack::new("heroku", "jvm-function-invoker");
    let buildpack_dir = setup_buildpack_dir(&buildpack, &buildpacks_dir).await;

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
