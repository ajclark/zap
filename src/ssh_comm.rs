use std::process::Command;
use std::path::Path;
use std::fs::File;
use std::io::{self, Read, Write, Seek};
use std::time::{Duration, Instant};
use std::thread;
use indicatif::ProgressBar;

const RETRY_DELAY_SECONDS: u64 = 5;
const BUFFER_SIZE: usize = 1 * 1024 * 1024; // 1MB

pub fn stream_stream_from_remote(
    stream_num: usize,
    start: usize,
    end: usize,
    remote_file: &str,
    remote_user: &str,
    remote_host: &str,
    local_path: &str,
    ssh_key_path: Option<&str>,
    retries: u32,
    ssh_port: usize,
    pb: ProgressBar,
) -> Result<(), String> {
    let mut attempt = 0;
    let ssh_port_str = ssh_port.to_string();
    let bytes_to_read = end - start;

    while attempt <= retries {
        let user_host = format!("{}@{}", remote_user, remote_host);
        let stream_command = format!(
            "dd if={} bs={} skip={} count={} status=none",
            remote_file,
            BUFFER_SIZE,
            start / BUFFER_SIZE,
            (bytes_to_read + BUFFER_SIZE - 1) / BUFFER_SIZE
        );
        
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

        let mut child = match Command::new("ssh")
            .args(&ssh_args)
            .stdout(std::process::Stdio::piped())
            .spawn() {
                Ok(child) => child,
                Err(e) => {
                    eprintln!("Failed to spawn SSH process: {}", e);
                    attempt += 1;
                    if attempt > retries {
                        return Err(format!("Failed to spawn SSH after {} retries", retries));
                    }
                    thread::sleep(Duration::from_secs(RETRY_DELAY_SECONDS));
                    continue;
                }
            };

        let result = (|| -> io::Result<()> {
            if let Some(mut stdout) = child.stdout.take() {
                let stream_path = format!("{}/stream_{}.bin", local_path, stream_num);
                let mut file = File::create(&stream_path)?;
                let mut total_read = 0;
                let mut buffer = vec![0u8; BUFFER_SIZE];
                let start_time = Instant::now();
                let mut last_update_time = start_time;

                loop {
                    match stdout.read(&mut buffer) {
                        Ok(0) => break, // EOF
                        Ok(n) => {
                            let write_size = std::cmp::min(n, bytes_to_read - total_read);
                            file.write_all(&buffer[..write_size])?;
                            total_read += write_size;

                            // Update progress
                            pb.set_position(total_read as u64);

                            // Update throughput display
                            let current_time = Instant::now();
                            if current_time.duration_since(last_update_time) > Duration::from_secs(1) {
                                let duration = current_time.duration_since(start_time);
                                let duration_secs = duration.as_secs() as f64 + duration.subsec_nanos() as f64 * 1e-9;
                                let throughput = (total_read as f64 / 1024.0 / 1024.0) / duration_secs;
                                pb.set_message(format!("{:.2} MB/s", throughput));
                                last_update_time = current_time;
                            }

                            if total_read >= bytes_to_read {
                                break;
                            }
                        },
                        Err(e) => return Err(e),
                    }
                }

                if total_read == bytes_to_read {
                    pb.finish_with_message("done");
                    Ok(())
                } else {
                    pb.finish_with_message("incomplete");
                    Err(io::Error::new(io::ErrorKind::UnexpectedEof, 
                        format!("Transfer incomplete: {} of {} bytes", total_read, bytes_to_read)))
                }
            } else {
                Err(io::Error::new(io::ErrorKind::Other, "Failed to get stdout handle"))
            }
        })();

        match result {
            Ok(_) => {
                match child.wait() {
                    Ok(status) if status.success() => return Ok(()),
                    Ok(_) => {
                        eprintln!("SSH process exited with non-zero status for stream {}", stream_num);
                        attempt += 1;
                    },
                    Err(e) => {
                        eprintln!("Error waiting for SSH process: {}", e);
                        attempt += 1;
                    }
                }
            },
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
    let mut attempt = 0;
    let ssh_port_str = ssh_port.to_string();
    let bytes_to_transfer = end - start;

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

        let mut child = match Command::new("ssh")
            .args(&ssh_args)
            .stdin(std::process::Stdio::piped())
            .spawn() {
                Ok(child) => child,
                Err(e) => {
                    eprintln!("Failed to spawn SSH process: {}", e);
                    attempt += 1;
                    if attempt > retries {
                        return Err(format!("Failed to spawn SSH after {} retries", retries));
                    }
                    thread::sleep(Duration::from_secs(RETRY_DELAY_SECONDS));
                    continue;
                }
            };

        let result = (|| -> io::Result<()> {
            if let Some(mut stdin) = child.stdin.take() {
                let mut file = File::open(input_file)?;
                file.seek(io::SeekFrom::Start(start as u64))?;
                let mut buffer = vec![0; BUFFER_SIZE];
                let mut total_written = 0;

                let start_time = Instant::now();
                let mut last_update_time = start_time;

                while total_written < bytes_to_transfer {
                    let to_read = std::cmp::min(BUFFER_SIZE, bytes_to_transfer - total_written);
                    let bytes_read = file.read(&mut buffer[..to_read])?;
                    if bytes_read == 0 {
                        break;  // EOF
                    }

                    stdin.write_all(&buffer[..bytes_read])?;
                    total_written += bytes_read;

                    // Update progress bar
                    pb.set_position(total_written as u64);

                    // Update throughput every second
                    let current_time = Instant::now();
                    if current_time.duration_since(last_update_time) > Duration::from_secs(1) {
                        let duration = current_time.duration_since(start_time);
                        let duration_secs = duration.as_secs() as f64 + duration.subsec_nanos() as f64 * 1e-9;
                        let throughput = (total_written as f64 / 1024.0 / 1024.0) / duration_secs;
                        pb.set_message(format!("{:.2} MB/s", throughput));
                        last_update_time = current_time;
                    }
                }
                stdin.flush()?;
                drop(stdin);  // Explicitly close stdin

                if total_written == bytes_to_transfer {
                    pb.finish_with_message("done");
                    Ok(())
                } else {
                    pb.finish_with_message("incomplete");
                    Err(io::Error::new(io::ErrorKind::UnexpectedEof, 
                        format!("Transfer incomplete: {} of {} bytes", total_written, bytes_to_transfer)))
                }
            } else {
                Err(io::Error::new(io::ErrorKind::Other, "Failed to get stdin handle"))
            }
        })();

        match result {
            Ok(_) => {
                match child.wait() {
                    Ok(status) if status.success() => return Ok(()),
                    Ok(_) => {
                        eprintln!("SSH process exited with non-zero status for stream {}", stream_num);
                        attempt += 1;
                    },
                    Err(e) => {
                        eprintln!("Error waiting for SSH process: {}", e);
                        attempt += 1;
                    }
                }
            },
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

pub fn assemble_local_streams(
    local_path: &str,
    num_streams: usize,
    output_file: &str,
) -> io::Result<()> {
    println!("Assembling {} streams into {}", num_streams, output_file);
    let mut output = File::create(output_file)?;
    
    for i in 0..num_streams {
        let stream_path = format!("{}/stream_{}.bin", local_path, i);
        let mut stream_file = File::open(&stream_path)?;
        io::copy(&mut stream_file, &mut output)?;
        std::fs::remove_file(&stream_path)?;
    }
    
    Ok(())
}

pub fn assemble_streams(
    remote_user: &str,
    remote_host: &str,
    remote_path: &str,
    ssh_key_path: Option<&str>,
    num_streams: usize,
    input_file: &str,
    ssh_port: usize,
) {
    let file_name = Path::new(input_file)
        .file_name()
        .expect("Invalid input file path")
        .to_str()
        .expect("Invalid file name");

    println!("Assembling {} streams on remote host", num_streams);

    let remove_existing_file_command = format!("rm -f {}/{}", remote_path, file_name);

    let assemble_command: Vec<String> = (0..num_streams)
        .map(|i| format!("cat {}/stream_{}.bin >> \"{}/{}\" && rm {}/stream_{}.bin", 
             remote_path, i, remote_path, file_name, remote_path, i))
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
