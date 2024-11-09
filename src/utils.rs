use std::sync::{Arc, Mutex};
use std::thread;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;
use std::time::Instant;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use crate::ssh_comm::{stream_stream_to_remote, stream_stream_from_remote, assemble_streams, assemble_local_streams};

struct TransferStats {
    start_time: Instant,
    total_bytes: usize,
    streams_completed: usize,
}

fn format_speed(bytes_per_second: f64) -> String {
    if bytes_per_second >= 1_000_000_000.0 {
        format!("{:.2} GB/s", bytes_per_second / 1_000_000_000.0)
    } else if bytes_per_second >= 1_000_000.0 {
        format!("{:.2} MB/s", bytes_per_second / 1_000_000.0)
    } else if bytes_per_second >= 1_000.0 {
        format!("{:.2} KB/s", bytes_per_second / 1_000.0)
    } else {
        format!("{:.2} B/s", bytes_per_second)
    }
}

fn format_size(bytes: usize) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.2} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.2} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.2} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{} B", bytes)
    }
}

fn print_transfer_stats(stats: &TransferStats, num_streams: usize) {
    let duration = stats.start_time.elapsed();
    let duration_secs = duration.as_secs() as f64 + duration.subsec_nanos() as f64 * 1e-9;
    let speed = stats.total_bytes as f64 / duration_secs;
    
    println!("\nTransfer Statistics");
    println!("Total Size:    {}", format_size(stats.total_bytes));
    println!("Streams:       {}", num_streams);
    println!("Duration:      {:.2} seconds", duration_secs);
    println!("Average Speed: {}", format_speed(speed));
}

fn get_remote_file_size(
    remote_file: &str,
    remote_user: &str,
    remote_host: &str,
    ssh_port: usize,
    ssh_key_path: Option<&str>,
) -> io::Result<usize> {
    let port_str = ssh_port.to_string();
    let user_host = format!("{}@{}", remote_user, remote_host);
    
    // Use wc -c which is portable across Unix-like systems
    let size_command = format!("wc -c < {}", remote_file);
    
    let mut ssh_args = vec![
        "-p", &port_str,
        "-o", "StrictHostKeyChecking=no",
        &user_host,
        &size_command,
    ];

    if let Some(key_path) = ssh_key_path {
        ssh_args.insert(0, key_path);
        ssh_args.insert(0, "-i");
    }

    let output = Command::new("ssh")
        .args(&ssh_args)
        .output()?;

    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("Failed to get remote file size. File may not exist or permission denied: {}", 
                   String::from_utf8_lossy(&output.stderr))
        ));
    }

    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}

pub fn split_and_copy_binary_file(
    quiet_mode: bool,
    input_file: &str,
    num_streams: usize,
    remote_user: &str,
    remote_host: &str,
    remote_path: &str,
    ssh_key_path: Option<&str>,
    max_threads: usize,
    retries: u32,
    ssh_port: usize,
) {
    if !quiet_mode {
        println!("Preparing to transfer {}...", input_file);
    }

    let file_size = match fs::metadata(input_file) {
        Ok(metadata) => metadata.len() as usize,
        Err(e) => {
            eprintln!("Error reading local file: {}", e);
            return;
        }
    };

    let stats = Arc::new(Mutex::new(TransferStats {
        start_time: Instant::now(),
        total_bytes: file_size,
        streams_completed: 0,
    }));

    if !quiet_mode {
        println!("Local file size: {} ({})", format_size(file_size), file_size);

        let stream_size = file_size / num_streams;
        println!("Using {} streams of approximately {} each", 
                 num_streams,
                 format_size(stream_size));

        let extra_bytes = file_size % num_streams;
        if extra_bytes > 0 {
            println!("Last stream will have an additional {} bytes", extra_bytes);
        }

        println!("Initializing transfer...");
    }

    let retry_flag = Arc::new(Mutex::new(vec![false; num_streams]));
    let mut handles = Vec::with_capacity(max_threads);

    let stream_size = file_size / num_streams;
    let extra_bytes = file_size % num_streams;

    let m = if !quiet_mode { MultiProgress::new() } else { MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden()) };
    let style = ProgressStyle::with_template(
        "[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}",
    )
    .unwrap()
    .progress_chars("##-");

    for stream_num in 0..num_streams {
        let input_file = input_file.to_string();
        let remote_user = remote_user.to_string();
        let remote_host = remote_host.to_string();
        let remote_path = remote_path.to_string();
        let ssh_key_path_cloned = ssh_key_path.map(|s| s.to_string());
        let retry_flag_clone = Arc::clone(&retry_flag);
        let stats_clone = Arc::clone(&stats);

        let pb = m.add(ProgressBar::new(stream_size as u64));
        pb.set_style(style.clone());
        pb.set_message(format!("Stream {}", stream_num));

        let handle = thread::spawn(move || {
            let start = stream_num * stream_size;
            let mut end = start + stream_size;

            if stream_num == num_streams - 1 {
                end += extra_bytes;
            }

            match stream_stream_to_remote(
                stream_num,
                start,
                end,
                &input_file,
                &remote_user,
                &remote_host,
                &remote_path,
                ssh_key_path_cloned.as_deref(),
                retries,
                ssh_port,
                pb
            ) {
                Ok(_) => {
                    let mut stats = stats_clone.lock().unwrap();
                    stats.streams_completed += 1;
                },
                Err(e) => {
                    eprintln!("{}", e);
                    let mut flags = retry_flag_clone.lock().unwrap();
                    flags[stream_num] = true;
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        let _ = handle.join();
    }

    let flags = retry_flag.lock().unwrap();
    if flags.iter().any(|&flag| flag) {
        eprintln!("Some streams failed to transfer and need to be retried.");
    } else {
        println!("All streams transferred successfully. Assembling on remote host...");
        assemble_streams(
            remote_user,
            remote_host,
            remote_path,
            ssh_key_path,
            num_streams,
            input_file,
            ssh_port
        );
        
        // Print final statistics
        let stats = stats.lock().unwrap();
        print_transfer_stats(&stats, num_streams);
    }
}

pub fn split_and_copy_from_remote(
    quiet_mode: bool,
    remote_file: &str,
    num_streams: usize,
    remote_user: &str,
    remote_host: &str,
    local_path: &str,
    ssh_key_path: Option<&str>,
    max_threads: usize,
    retries: u32,
    ssh_port: usize,
) -> io::Result<()> {
    if !quiet_mode {
        println!("Preparing to transfer {}...", remote_file);
    }
    
    let file_size = get_remote_file_size(
        remote_file,
        remote_user,
        remote_host,
        ssh_port,
        ssh_key_path,
    )?;

    let stats = Arc::new(Mutex::new(TransferStats {
        start_time: Instant::now(),
        total_bytes: file_size,
        streams_completed: 0,
    }));

    if !quiet_mode {
        println!("Remote file size: {} ({})", format_size(file_size), file_size);

        let stream_size = file_size / num_streams;
        println!("Using {} streams of approximately {} each", 
                 num_streams,
                 format_size(stream_size));

        let extra_bytes = file_size % num_streams;
        if extra_bytes > 0 {
            println!("Last stream will have an additional {} bytes", extra_bytes);
        }

        println!("Initializing transfer...");
    }
    
    let retry_flag = Arc::new(Mutex::new(vec![false; num_streams]));
    let mut handles = Vec::with_capacity(max_threads);

    let stream_size = file_size / num_streams;
    let extra_bytes = file_size % num_streams;

    let m = if !quiet_mode { MultiProgress::new() } else { MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden()) };
    let style = ProgressStyle::with_template(
        "[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}",
    )
    .unwrap()
    .progress_chars("##-");

    for stream_num in 0..num_streams {
        let remote_file = remote_file.to_string();
        let remote_user = remote_user.to_string();
        let remote_host = remote_host.to_string();
        let local_path = local_path.to_string();
        let ssh_key_path_cloned = ssh_key_path.map(|s| s.to_string());
        let retry_flag_clone = Arc::clone(&retry_flag);
        let stats_clone = Arc::clone(&stats);

        let pb = m.add(ProgressBar::new(stream_size as u64));
        pb.set_style(style.clone());
        pb.set_message(format!("Stream {}", stream_num));

        let handle = thread::spawn(move || {
            let start = stream_num * stream_size;
            let mut end = start + stream_size;

            if stream_num == num_streams - 1 {
                end += extra_bytes;
            }

            match stream_stream_from_remote(
                stream_num,
                start,
                end,
                &remote_file,
                &remote_user,
                &remote_host,
                &local_path,
                ssh_key_path_cloned.as_deref(),
                retries,
                ssh_port,
                pb
            ) {
                Ok(_) => {
                    let mut stats = stats_clone.lock().unwrap();
                    stats.streams_completed += 1;
                },
                Err(e) => {
                    eprintln!("{}", e);
                    let mut flags = retry_flag_clone.lock().unwrap();
                    flags[stream_num] = true;
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        let _ = handle.join();
    }

    let flags = retry_flag.lock().unwrap();
    if flags.iter().any(|&flag| flag) {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "Some streams failed to transfer and need to be retried."
        ))
    } else {
        println!("All streams transferred successfully. Assembling locally...");
        let output_file = Path::new(remote_file)
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Invalid remote file path"))?
            .to_str()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Invalid file name"))?;
        let output_path = Path::new(local_path).join(output_file);
        assemble_local_streams(local_path, num_streams, output_path.to_str().unwrap())?;
        
        // Print final statistics
        let stats = stats.lock().unwrap();
        print_transfer_stats(&stats, num_streams);
        
        Ok(())
    }
}
