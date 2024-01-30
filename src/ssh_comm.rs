use std::process::Command;
use std::path::Path;
use std::fs::File;
use std::io::{self, Read, Write, Seek};
use std::time::{Duration, Instant};
use std::thread;
use indicatif::ProgressBar;

const RETRY_DELAY_SECONDS: u64 = 5;
const BUFFER_SIZE: usize = 1 * 1024 * 1024; // 1MB

pub fn stream_stream_to_remote(
    stream_num: usize,
    start: usize,
    end: usize,
    input_file: &str,
    remote_user: &str,
    remote_host: &str,
    remote_path: &str,
    ssh_key_path: Option<&str>,
    retries: u32,
    ssh_port: usize,
    pb: ProgressBar,
) -> Result<(), String> {

    // 100-750ms delay to prevent DoSing remote SSH daemon
    let delay = (stream_num * 123 % 651) + 100;
    thread::sleep(Duration::from_millis(delay as u64));

    let mut attempt = 0;
    let ssh_port_str = ssh_port.to_string();

    while attempt <= retries {
        let user_host = format!("{}@{}", remote_user, remote_host);
        let stream_command = format!("cat > {}/stream_{}.bin", remote_path, stream_num);
        let mut ssh_args = vec![
            "-p", &ssh_port_str,
            "-o", "StrictHostKeyChecking=no",
            &user_host,
            &stream_command,
        ];

        if let Some(key_path) = ssh_key_path {
            ssh_args.insert(0, key_path);
            ssh_args.insert(0, "-i");
        }

        let mut child = Command::new("ssh")
            .args(&ssh_args)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .expect("Failed to start ssh command");

        let result = (|| -> io::Result<()> {
            if let Some(mut stdin) = child.stdin.take() {
                let mut file = File::open(input_file)?;
                file.seek(io::SeekFrom::Start(start as u64))?;
                let mut buffer = vec![0; BUFFER_SIZE.min(end - start)];
                let mut bytes_to_read = end - start;

                let start_time = Instant::now();
                let mut last_update_time = start_time;
                while bytes_to_read > 0 {
                    let read_size = buffer.len().min(bytes_to_read);
                    let bytes_read = file.read(&mut buffer[..read_size])?;
                    stdin.write_all(&buffer[..bytes_read])?;
                    bytes_to_read -= bytes_read;

                    // Update progress bar
                    pb.set_position((end - start - bytes_to_read) as u64);

                    // Update throughput every second or so
                    let current_time = Instant::now();
                    if current_time.duration_since(last_update_time) > Duration::from_secs(1) {
                        let duration = current_time.duration_since(start_time);
                        let duration_in_seconds = duration.as_secs() as f64 
                                                + duration.subsec_nanos() as f64 * 1e-9;
                        let throughput = ((end - start - bytes_to_read) as f64 / 1024.0 / 1024.0) / duration_in_seconds;
                        pb.set_message(format!("{:.2} MB/s", throughput));
                        last_update_time = current_time;
                    }
                }
                stdin.flush()?;

            }

            let output = child.wait_with_output()?;
            if output.status.success() {
                pb.finish_with_message("done");
                Ok(())
            } else {
                pb.finish_with_message("error");
                Err(io::Error::new(io::ErrorKind::Other, "SSH command failed"))
            }
        })();

        match result {
            Ok(_) => return Ok(()),
            Err(e) => {
                eprintln!("Error streaming stream {}: {}", stream_num, e);
                attempt += 1;
                if attempt > retries {
                    pb.finish_with_message("failed");
                    return Err(format!("Failed to stream stream {} after {} retries", stream_num, retries));
                }
                eprintln!("Retrying stream {} ({}/{})", stream_num, attempt, retries);
                thread::sleep(Duration::from_secs(RETRY_DELAY_SECONDS));
            }
        }
    }

    Err(format!("Failed to stream stream {} after {} retries", stream_num, retries))
}

pub fn assemble_streams(
    remote_user: &str,
    remote_host: &str,
    remote_path: &str,
    ssh_key_path: Option<&str>,
    num_streams: usize,
    input_file_path: &str,
    ssh_port: usize,
) {
    let file_name = Path::new(input_file_path)
        .file_name()
        .expect("Invalid input file path")
        .to_str()
        .expect("Invalid file name");

    let remove_existing_file_command = format!("rm -f {}/{}", remote_path, file_name);

    let assemble_command: Vec<String> = (0..num_streams)
        .map(|i| format!("cat {}/stream_{}.bin >> {}/{} && rm {}/stream_{}.bin", remote_path, i, remote_path, file_name, remote_path, i))
        .collect();

    let ssh_key_arg = ssh_key_path.map_or_else(|| "".to_string(), |key| format!("-i {}", key));
    let ssh_command = format!(
        "ssh -p {} {} -o StrictHostKeyChecking=no {}@{} '{}; {};'",
        ssh_port,
        ssh_key_arg,
        remote_user,
        remote_host,
        remove_existing_file_command,
        assemble_command.join(";")
    );

    Command::new("sh")
        .arg("-c")
        .arg(&ssh_command)
        .status()
        .expect("Failed to execute ssh command to assemble and clean up streams");

    println!("File assembled and streams cleaned on {}:{}/{}", remote_host, remote_path, file_name);
}

