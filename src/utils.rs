use std::sync::{Arc, Mutex};
use std::thread;
use std::fs;
use crate::ssh_comm::{stream_stream_to_remote, assemble_streams};

pub fn split_and_copy_binary_file(
    input_file: &str,
    num_streams: usize,
    remote_user: &str,
    remote_host: &str,
    remote_path: &str,
    ssh_key_path: Option<&str>,
    max_threads: usize,
) {
    let file_size = fs::metadata(input_file).unwrap().len() as usize;
    let stream_size = file_size / num_streams;
    let extra_bytes = file_size % num_streams; // Remaining bytes for the last stream
    let retry_flag = Arc::new(Mutex::new(vec![false; num_streams]));
    let mut handles = Vec::with_capacity(max_threads);

    for stream_num in 0..num_streams {
        let input_file = input_file.to_string();
        let remote_user = remote_user.to_string();
        let remote_host = remote_host.to_string();
        let remote_path = remote_path.to_string();
        let ssh_key_path_cloned = ssh_key_path.map(|s| s.to_string());
        let retry_flag_clone = Arc::clone(&retry_flag);

        let handle = thread::spawn(move || {
            let start = stream_num * stream_size;
            let mut end = start + stream_size;

            // Add any extra bytes to the last stream
            if stream_num == num_streams - 1 {
                end += extra_bytes;
            }

            match stream_stream_to_remote(
                stream_num, start, end, &input_file, &remote_user, &remote_host, &remote_path, ssh_key_path_cloned.as_deref()
            ) {
                Ok(_) => println!("Chunk {} transferred successfully.", stream_num),
                Err(e) => {
                    eprintln!("{}", e);
                    let mut flags = retry_flag_clone.lock().unwrap();
                    flags[stream_num] = true;
                }
            }
        });

        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        let _ = handle.join(); // Ignoring the result
    }

    // Check if any streams failed and need to be retried
    let flags = retry_flag.lock().unwrap();
    if flags.iter().any(|&flag| flag) {
        eprintln!("Some streams failed to transfer and need to be retried.");
    } else {
        println!("All streams transferred successfully. Assembling on remote host...");
        assemble_streams(&remote_user, &remote_host, &remote_path, ssh_key_path, num_streams, &input_file);
    }
}
