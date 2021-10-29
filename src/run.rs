//run.sh implementation in rust

use cmd_lib::*;
use std::io;
use coresight::debug_port;

fn main() -> CmdResult {

    // do not fail due to pipe errors
    cmd_lib::set_pipefail(false); 
    
    // get paths
    let args: Vec<String> = env::args().collect();
    let mut _runtime_layer_jar_path = args[0];
    let mut _function_bundle_layer_dir = args[1];
    // println!("{:?}", args);

    let mut additional_java_args = String::new();
    if(debug_port.to_string().is_empty().not()) {
        // get the java version 
        let java_version= run_fun!(
            bash -c "java -version 2>&1"
        )?;
        let mut additional_java = String::new();
        if java_version.contains("1.8") {
            additional_java_args = additional_java_arg
                .join("-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=")
                .join(debug_port.to_string());
        } else {
            additional_java_args = additional_java_arg
                .join("-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=*:")
                .join(debug_port.to_string());
        }
    }

    // execute the java and jar serve

    let java_base = "exec java ";
    if(additional_java_args.is_empty()) {
        let no_additional_java_args = java_base
            .join("\ -jar ")
            .join(_runtime_layer_jar_path)
            .join("serve ")
            .join(_function_bundle_layer_dir)
            .join(" -h 0.0.0.0 -p "${PORT:-8080}"");
        run_cmb!(no_additional_java_args)?;
    } else {
        let with_additional_java_args = java_base
            .join(additional_java_args)
            .join(" \ -jar ")
            .join(_runtime_layer_jar_path)
            .join("serve ")
            .join(_function_bundle_layer_dir)
            .join(" -h 0.0.0.0 -p "${PORT:-8080}"");

        run_cmb!(with_additional_java_args)?;
    }

    Ok(())
}




