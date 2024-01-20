mod ssh_comm;
mod utils;

use clap::{App, Arg};
use utils::split_and_copy_binary_file;
use std::env;
use std::process;

fn main() {
    let matches = App::new("Zap — Fast single file copy")
        .version("0.1")
        .author("Allan Clark. <napta2k@gmail.com>")
        .about("Transfers a file in parallel streams over SSH")
        .arg_required_else_help(true)
        .arg(Arg::new("input_file")
            .help("The input file path")
            .required(true)
            .index(1))
        .arg(Arg::new("user_host_path")
            .help("Specifies user@host:remote_path")
            .required(true)
            .value_name("user@host:remote_path")
            .index(2))
        .arg(Arg::new("streams")
            .short('c')
            .long("streams")
            .help("The number of parallel streams")
            .default_value("20")
            .takes_value(true))
        .arg(Arg::new("ssh_key_path")
            .short('i')
            .long("ssh-key-path")
            .help("The SSH key path for authentication")
            .takes_value(true))
        .arg(Arg::new("retries")
            .short('r')
            .long("retries")
            .help("The number of retries to attempt")
            .takes_value(true)
            .default_value("3"))
        .arg(Arg::new("port")
            .short('p')
            .long("port")
            .help("SSH port")
            .takes_value(true)
            .required(false)
            .default_value("22"))
        .get_matches();

    let user_host_path = matches.value_of("user_host_path").unwrap();
    
    // Split into user@host and remote_path
    let parts: Vec<&str> = user_host_path.splitn(2, ':').collect();
    if parts.len() != 2 {
        eprintln!("Invalid format: Expected format user@host:remote_file, host:remote_file, or host:");
        process::exit(1);
    }

    let user_host = parts[0];
    let mut remote_path = parts[1].to_string();
    if remote_path.is_empty() {
        remote_path = ".".to_string();
    }

    // Disallow empty user with @ present
    if user_host.starts_with('@') {
        eprintln!("Invalid format: '@host:' is not allowed");
        process::exit(1);
    }

    // Disallow empty host with @ present
    if user_host.ends_with('@') {
        eprintln!("Invalid format: 'user@:' is not allowed");
        process::exit(1);
    }

    let host_parts: Vec<&str> = user_host.split('@').collect();
    let (remote_user, remote_host) = match host_parts.as_slice() {
        [user, host] => (user.to_string(), host.to_string()),
        [host] => {
            let user_env = env::var("USER").unwrap_or_else(|_| {
                eprintln!("$USER environment variable is not set");
                process::exit(1);
            });
            (user_env, host.to_string())
        },
        _ => {
            eprintln!("Invalid format for user@host:remote_path");
            process::exit(1);
        }
    };

    let input_file_path = matches.value_of("input_file").unwrap();
    let num_streams: usize = matches.value_of("streams").unwrap().parse()
        .expect("num_streams must be an integer");
    let ssh_port: usize = matches.value_of("port").unwrap().parse()
        .expect("port must be an integer");
    let ssh_key_path = matches.value_of("ssh_key_path");
    let retries: u32 = matches.value_of("retries").unwrap().parse()
        .expect("retries must be an integer");
    let max_threads = num_streams;

    split_and_copy_binary_file(
        input_file_path, 
        num_streams, 
        &remote_user, 
        &remote_host, 
        &remote_path,
        ssh_key_path.as_deref(),
        max_threads,
        retries,
        ssh_port,
    );
}

