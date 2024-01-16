mod ssh_comm;
mod utils;

use clap::{App, Arg};
use utils::split_and_copy_binary_file;

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
        .arg(Arg::new("streams")
            .short('c')
            .long("streams")
            .help("The number of parallel streams")
            .default_value("20")
            .takes_value(true))
        .arg(Arg::new("user")
            .short('u')
            .long("user")
            .help("The username for the remote server")
            .takes_value(true)
            .required(true))
        .arg(Arg::new("server")
            .short('s')
            .long("server")
            .help("The hostname of the remote server")
            .takes_value(true)
            .required(true))
        .arg(Arg::new("remote_path")
            .short('p')
            .long("remote-path")
            .help("The remote path where streams will be stored")
            .takes_value(true)
            .required(true))
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
        .get_matches();

    let input_file_path = matches.value_of("input_file").unwrap();
    let num_streams: usize = matches.value_of("streams").unwrap().parse()
        .expect("num_streams must be an integer");
    let remote_user = matches.value_of("user").unwrap();
    let remote_host = matches.value_of("server").unwrap();
    let remote_path = matches.value_of("remote_path").unwrap();
    let ssh_key_path = matches.value_of("ssh_key_path");
    let retries: u32 = matches.value_of("retries").unwrap().parse()
        .expect("retries must be an integer");
    let max_threads = num_streams;

    split_and_copy_binary_file(
        input_file_path, 
        num_streams, 
        remote_user, 
        remote_host, 
        remote_path, 
        ssh_key_path.as_deref(),
        max_threads,
        retries
    );
}
