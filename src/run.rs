//run.sh implementation in rust

use cmd_lib::*;
use std::io;
use coresight::debug_port;
use std::env;

fn main() -> CmdResult {

    // do not fail due to pipe errors
    cmd_lib::set_pipefail(false); 
    
    // get paths
    let args: Vec<String> = env::args().collect();
    // let mut _runtime_layer_jar_path = args[0];
    // let mut _function_bundle_layer_dir = args[1];
    // println!("{:?}", args);

    let mut additional_java_args = String::new();
    let test_string = String::new();
    if test_string.is_empty() {
        // get the java version 
        let java_version= run_fun!(
            bash -c "java -version 2>&1"
        )?;
        let mut additional_java = String::new();
        if java_version.contains("1.8") {
            additional_java_args.push_str("-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=");
                // additional_java_args.push(debug_port.to_string());
        } else {
            additional_java_args.push_str("-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=*:");
                // additional_java_args.push(debug_port.to_string());
        }
    }

    // execute the java and jar serve

    if additional_java_args.is_empty() {
        let mut no_additional_java_args = String::new();
        no_additional_java_args.push_str("exec java");
        no_additional_java_args.push_str(r#" / "#);
        no_additional_java_args.push_str(" -jar ");
        // no_additional_java_args.push_str(&_runtime_layer_jar_path);
        no_additional_java_args.push_str("serve ");
        // no_additional_java_args.push_str(&_function_bundle_layer_dir);
        no_additional_java_args.push_str(" -h 0.0.0.0 -p ");
        no_additional_java_args.push_str(r#"${PORT:-8080}""#);

        // run_cmd!(no_additional_java_args)?;
        println!("{}",no_additional_java_args);
    } else {
        let mut with_additional_java_args = String::new();
        with_additional_java_args.push_str("exec java ");
        with_additional_java_args.push_str(&additional_java_args);
        with_additional_java_args.push_str(r#" / "#);
        with_additional_java_args.push_str(" -jar ");
        // with_additional_java_args.push_str(&_runtime_layer_jar_path);
        with_additional_java_args.push_str("serve ");
        // with_additional_java_args.push_str(&_function_bundle_layer_dir);
        with_additional_java_args.push_str(" -h 0.0.0.0 -p ");
        with_additional_java_args.push_str(r#"${PORT:-8080}""#);

        // run_cmd!(with_additional_java_args)?;
        println!("{}",with_additional_java_args);
    }

    Ok(())
}




