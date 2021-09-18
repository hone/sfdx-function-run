use libcnb::data::launch::ProcessType;
use std::{
    fs,
    path::{Path, PathBuf},
    process,
};

const DEFAULT_STACK_ID: &str = "io.buildpacks.stacks.bionic";

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

fn main() {
    let buildpack_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("heroku_jvm-function-invoker")
        .join("cnb")
        .join("buildpacks")
        .join("heroku_jvm-function-invoker")
        .join("0.5.2");

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

    for dir in [&platform_dir, &layers_dir, &tmp_dir] {
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
        process::Command::new(&process.command)
            .args(&process.args)
            .stdin(process::Stdio::null())
            .stdout(process::Stdio::inherit())
            .spawn()
            .unwrap();
    }
}
