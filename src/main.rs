mod ssh_comm;
mod utils;

use clap::{App, Arg};
use utils::{split_and_copy_binary_file, split_and_copy_from_remote};
use std::env;
use std::process;
use std::path::Path;

fn parse_location(loc: &str) -> Option<(Option<(String, String)>, String)> {
    if loc.contains(':') {
        let parts: Vec<&str> = loc.splitn(2, ':').collect();
        if parts.len() != 2 {
            return None;
        }

        let user_host = parts[0];
        let mut path = parts[1].to_string();
        
        // If path is empty default to $CWD
        if path.is_empty() {
            path = ".".to_string();
        }

        // Disallow empty user with @ present
        if user_host.starts_with('@') {
            return None;
        }

        // Disallow empty host with @ present
        if user_host.ends_with('@') {
            return None;
        }

        let host_parts: Vec<&str> = user_host.split('@').collect();
        match host_parts.as_slice() {
            [user, host] => Some((Some((user.to_string(), host.to_string())), path)),
            [host] => {
                let user = env::var("USER").ok()?;
                Some((Some((user, host.to_string())), path))
            },
            _ => None
        }
    } else {
        Some((None, loc.to_string()))
    }
}

fn validate_paths(source: &str, destination: &str) -> Result<(), String> {
    let (source_remote, source_path) = parse_location(source)
        .ok_or_else(|| "Invalid source format. Expected either a local path or user@host:path".to_string())?;
    
    let (dest_remote, dest_path) = parse_location(destination)
        .ok_or_else(|| "Invalid destination format. Expected either a local path or user@host:path".to_string())?;

    // Check that exactly one location is remote
    match (source_remote.is_some(), dest_remote.is_some()) {
        (true, true) => {
            Err("Cannot copy from remote to remote".to_string())
        },
        (false, false) => {
            Err("At least one location must be remote".to_string())
        },
        _ => {
            // For local paths, verify they exist and are valid
            if source_remote.is_none() {
                let path = Path::new(&source_path);
                if !path.exists() {
                    return Err(format!("Source file '{}' does not exist", source_path));
                }
                if !path.is_file() {
                    return Err(format!("Source path '{}' is not a file", source_path));
                }
            }
            
            if dest_remote.is_none() {
                let path = Path::new(&dest_path);
                if !path.exists() {
                    return Err(format!("Destination directory '{}' does not exist", dest_path));
                }
                if !path.is_dir() {
                    return Err(format!("Destination path '{}' is not a directory", dest_path));
                }
            }
            
            Ok(())
        }
    }
}

fn main() {
    let matches = App::new("Zap")
        .version("v0.8.0-alpha")
        .author("Allan Clark. <napta2k@gmail.com>")
        .about("Transfers a file in parallel streams over SSH")
        .arg_required_else_help(true)
        .arg(Arg::new("source")
            .help("Source file (local file or user@host:remote_path)")
            .required(true)
            .index(1))
        .arg(Arg::new("destination")
            .help("Destination (local file or user@host:remote_path)")
            .required(true)
            .index(2))
        .arg(Arg::new("streams")
            .short('s')
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
        .after_help(
            "EXAMPLES:\n\
            \tPull a file from remote to local:\n\
            \t\tzap user@remote_host:/path/to/remote_file /local/destination/\n\
            \n\
            \tPush a file from local to remote:\n\
            \t\tzap /local/path/to/file user@remote_host:/remote/destination/\n"
        )
        .get_matches();

    let source = matches.value_of("source").unwrap();
    let destination = matches.value_of("destination").unwrap();

    // Validate source and destination paths
    if let Err(e) = validate_paths(source, destination) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }

    let (source_remote, source_path) = parse_location(source).unwrap();
    let (dest_remote, dest_path) = parse_location(destination).unwrap();

    // Parse common arguments
    let num_streams: usize = matches.value_of("streams").unwrap()
        .parse()
        .unwrap_or_else(|_| {
            eprintln!("Error: streams must be a positive integer");
            process::exit(1);
        });

    let ssh_port: usize = matches.value_of("port").unwrap()
        .parse()
        .unwrap_or_else(|_| {
            eprintln!("Error: port must be a positive integer");
            process::exit(1);
        });

    let retries: u32 = matches.value_of("retries").unwrap()
        .parse()
        .unwrap_or_else(|_| {
            eprintln!("Error: retries must be a positive integer");
            process::exit(1);
        });

    let ssh_key_path = matches.value_of("ssh_key_path");
    let max_threads = num_streams;

    match (source_remote, dest_remote) {
        (Some((remote_user, remote_host)), None) => {
            // Pull transfer
            if let Err(e) = split_and_copy_from_remote(
                &source_path,
                num_streams,
                &remote_user,
                &remote_host,
                &dest_path,
                ssh_key_path,
                max_threads,
                retries,
                ssh_port,
            ) {
                eprintln!("Error during pull transfer: {}", e);
                process::exit(1);
            }
        },
        (None, Some((remote_user, remote_host))) => {
            // Push transfer
            split_and_copy_binary_file(
                &source_path,
                num_streams,
                &remote_user,
                &remote_host,
                &dest_path,
                ssh_key_path,
                max_threads,
                retries,
                ssh_port,
            );
        },
        _ => {
            // This shouldn't happen due to validate_paths, but handle it anyway
            eprintln!("Error: Either source or destination must be remote, but not both");
            process::exit(1);
        }
    }
}
