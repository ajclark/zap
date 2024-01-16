use std::process::Command;
use std::path::Path;
use std::fs::File;
use std::io::{self, Read, Write, Seek};
use std::time::Duration;
use std::thread;

const MAX_RETRIES: u32 = 3;
const RETRY_DELAY_SECONDS: u64 = 5;
const BUFFER_SIZE: usize = 1 * 1024 * 1024; // 1MB

pub fn stream_chunk_to_remote(
    chunk_num: usize,
    start: usize,
    end: usize,
    input_file: &str,
    remote_user: &str,
    remote_host: &str,
    remote_path: &str,
    ssh_key_path: Option<&str>,
) -> Result<(), String> {
    let mut retries = 0;
    while retries <= MAX_RETRIES {
        let user_host = format!("{}@{}", remote_user, remote_host);
        let chunk_command = format!("cat > {}/chunk_{}.bin", remote_path, chunk_num);

        let mut ssh_args = vec![
            "-o", "StrictHostKeyChecking=no",
            &user_host,
            &chunk_command,
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
                let chunk_size = end - start; // Calculate the exact chunk size
                let mut buffer = vec![0; BUFFER_SIZE.min(chunk_size)];
                let mut bytes_to_read = chunk_size;

                while bytes_to_read > 0 {
                    let read_size = buffer.len().min(bytes_to_read);
                    file.read_exact(&mut buffer[..read_size])?;
                    stdin.write_all(&buffer[..read_size])?;
                    bytes_to_read -= read_size;
                }
                stdin.flush()?;
            }
            let output = child.wait_with_output()?;
            if output.status.success() {
                Ok(())
            } else {
                Err(io::Error::new(io::ErrorKind::Other, "SSH command failed"))
            }
        })();

        match result {
            Ok(_) => return Ok(()),
            Err(e) => {
                eprintln!("Error streaming chunk {}: {}", chunk_num, e);
                retries += 1;
                if retries > MAX_RETRIES {
                    return Err(format!("Failed to stream chunk {} after {} retries", chunk_num, MAX_RETRIES));
                }
                eprintln!("Retrying chunk {} ({}/{})", chunk_num, retries, MAX_RETRIES);
                thread::sleep(Duration::from_secs(RETRY_DELAY_SECONDS));
            }
        }
    }

    Err(format!("Failed to stream chunk {} after {} retries", chunk_num, MAX_RETRIES))
}

pub fn assemble_chunks(
    remote_user: &str,
    remote_host: &str,
    remote_path: &str,
    ssh_key_path: Option<&str>,
    num_chunks: usize,
    input_file_path: &str,
) {
    let file_name = Path::new(input_file_path)
        .file_name()
        .expect("Invalid input file path")
        .to_str()
        .expect("Invalid file name");

    // Command to remove the existing output file if it exists
    let remove_existing_file_command = format!("rm -f {}/{}", remote_path, file_name);

    // Command to assemble chunks into the final output file
    let assemble_command: Vec<String> = (0..num_chunks)
        .map(|i| format!("cat {}/chunk_{}.bin >> {}/{}", remote_path, i, remote_path, file_name))
        .collect();

    // Command to delete individual chunk files
    let delete_chunks_command: Vec<String> = (0..num_chunks)
        .map(|i| format!("rm {}/chunk_{}.bin", remote_path, i))
        .collect();

    let ssh_key_arg = ssh_key_path.map_or_else(|| "".to_string(), |key| format!("-i {}", key));
    let ssh_command = format!(
        "ssh {} -o StrictHostKeyChecking=no {}@{} '{}; {}; {}'",
        ssh_key_arg,
        remote_user,
        remote_host,
        remove_existing_file_command,
        assemble_command.join(";"),
        delete_chunks_command.join(";")
    );

    Command::new("sh")
        .arg("-c")
        .arg(&ssh_command)
        .status()
        .expect("Failed to execute ssh command to assemble and clean up chunks");

    println!("File assembled and chunks cleaned on {}:{}/{}", remote_host, remote_path, file_name);
}
