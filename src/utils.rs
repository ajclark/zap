use std::sync::{Arc, Mutex};
use std::thread;
use std::fs;
use std::io;
use std::path::Path;
use std::time::Instant;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use crate::ssh::{SessionConfig, connect_and_auth, open_sftp, stat_remote_file, extend_remote_file, pull_worker, push_worker};

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

/// Pull transfer: remote → local using SFTP
pub fn split_and_copy_from_remote(
    quiet_mode: bool,
    remote_file: &str,
    num_streams: usize,
    remote_user: &str,
    remote_host: &str,
    local_path: &str,
    ssh_key_path: Option<&str>,
    retries: u32,
    ssh_port: u16,
) -> io::Result<()> {
    if !quiet_mode {
        println!("Preparing to transfer {}...", remote_file);
    }

    // Create session config
    let cfg = SessionConfig {
        host: remote_host.to_string(),
        port: ssh_port,
        user: remote_user.to_string(),
        key_path: ssh_key_path.map(|s| s.to_string()),
        retries,
    };

    // Get remote file size
    let file_size = {
        let sess = connect_and_auth(&cfg)?;
        let sftp = open_sftp(&sess)?;
        stat_remote_file(&sftp, remote_file)?
    };

    let stats = Arc::new(Mutex::new(TransferStats {
        start_time: Instant::now(),
        total_bytes: file_size as usize,
        streams_completed: 0,
    }));

    if !quiet_mode {
        println!("Remote file size: {} ({})", format_size(file_size as usize), file_size);
        let stream_size = file_size / num_streams as u64;
        println!("Using {} streams of approximately {} each",
                 num_streams,
                 format_size(stream_size as usize));
        let extra_bytes = file_size % num_streams as u64;
        if extra_bytes > 0 {
            println!("Last stream will have an additional {} bytes", extra_bytes);
        }
        println!("Initializing transfer...");
    }

    // Determine output file path
    let file_name = Path::new(remote_file)
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Invalid remote file path"))?
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Invalid file name"))?;
    let output_path = Path::new(local_path).join(file_name);

    // Create local file and extend to full size (sparse)
    let local_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .read(true)
        .open(&output_path)?;
    local_file.set_len(file_size)?;
    let local_file = Arc::new(local_file);

    // Calculate segments
    let stream_size = file_size / num_streams as u64;
    let extra_bytes = file_size % num_streams as u64;

    // Setup progress bars
    let m = if !quiet_mode {
        MultiProgress::new()
    } else {
        MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden())
    };
    let style = ProgressStyle::with_template(
        "[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}",
    )
    .unwrap()
    .progress_chars("##-");

    let retry_flag = Arc::new(Mutex::new(vec![false; num_streams]));
    let mut handles = Vec::with_capacity(num_streams);

    // Spawn worker threads
    for stream_num in 0..num_streams {
        let cfg_clone = cfg.clone();
        let remote_file = remote_file.to_string();
        let local_file_clone = Arc::clone(&local_file);
        let retry_flag_clone = Arc::clone(&retry_flag);
        let stats_clone = Arc::clone(&stats);

        let start = stream_num as u64 * stream_size;
        let mut end = start + stream_size;
        if stream_num == num_streams - 1 {
            end += extra_bytes;
        }
        let segment_len = end - start;

        let pb = m.add(ProgressBar::new(segment_len));
        pb.set_style(style.clone());
        pb.set_message(format!("Stream {}", stream_num));

        let handle = thread::spawn(move || {
            match pull_worker(
                stream_num,
                start,
                end,
                &remote_file,
                &cfg_clone,
                &local_file_clone,
                pb,
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

    // Wait for all threads
    for handle in handles {
        let _ = handle.join();
    }

    // Check for failures
    let flags = retry_flag.lock().unwrap();
    if flags.iter().any(|&flag| flag) {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "Some streams failed to transfer after retries."
        ));
    }

    // Sync file to disk
    local_file.sync_all()?;

    println!("Transfer completed successfully!");

    // Print final statistics
    let stats = stats.lock().unwrap();
    print_transfer_stats(&stats, num_streams);

    Ok(())
}

/// Push transfer: local → remote using SFTP
pub fn split_and_copy_binary_file(
    quiet_mode: bool,
    input_file: &str,
    num_streams: usize,
    remote_user: &str,
    remote_host: &str,
    remote_path: &str,
    ssh_key_path: Option<&str>,
    retries: u32,
    ssh_port: u16,
) -> io::Result<()> {
    if !quiet_mode {
        println!("Preparing to transfer {}...", input_file);
    }

    // Get local file size
    let file_size = fs::metadata(input_file)?.len();

    let stats = Arc::new(Mutex::new(TransferStats {
        start_time: Instant::now(),
        total_bytes: file_size as usize,
        streams_completed: 0,
    }));

    if !quiet_mode {
        println!("Local file size: {} ({})", format_size(file_size as usize), file_size);
        let stream_size = file_size / num_streams as u64;
        println!("Using {} streams of approximately {} each",
                 num_streams,
                 format_size(stream_size as usize));
        let extra_bytes = file_size % num_streams as u64;
        if extra_bytes > 0 {
            println!("Last stream will have an additional {} bytes", extra_bytes);
        }
        println!("Initializing transfer...");
    }

    // Create session config
    let cfg = SessionConfig {
        host: remote_host.to_string(),
        port: ssh_port,
        user: remote_user.to_string(),
        key_path: ssh_key_path.map(|s| s.to_string()),
        retries,
    };

    // Determine remote file path
    let file_name = Path::new(input_file)
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Invalid input file path"))?
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Invalid file name"))?;
    let remote_file = format!("{}/{}", remote_path, file_name);

    // Create and extend remote file
    {
        let sess = connect_and_auth(&cfg)?;
        let sftp = open_sftp(&sess)?;
        extend_remote_file(&sftp, &remote_file, file_size)?;
    }

    // Calculate segments
    let stream_size = file_size / num_streams as u64;
    let extra_bytes = file_size % num_streams as u64;

    // Setup progress bars
    let m = if !quiet_mode {
        MultiProgress::new()
    } else {
        MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden())
    };
    let style = ProgressStyle::with_template(
        "[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}",
    )
    .unwrap()
    .progress_chars("##-");

    let retry_flag = Arc::new(Mutex::new(vec![false; num_streams]));
    let mut handles = Vec::with_capacity(num_streams);

    // Spawn worker threads
    for stream_num in 0..num_streams {
        let cfg_clone = cfg.clone();
        let input_file = input_file.to_string();
        let remote_file = remote_file.clone();
        let retry_flag_clone = Arc::clone(&retry_flag);
        let stats_clone = Arc::clone(&stats);

        let start = stream_num as u64 * stream_size;
        let mut end = start + stream_size;
        if stream_num == num_streams - 1 {
            end += extra_bytes;
        }
        let segment_len = end - start;

        let pb = m.add(ProgressBar::new(segment_len));
        pb.set_style(style.clone());
        pb.set_message(format!("Stream {}", stream_num));

        let handle = thread::spawn(move || {
            match push_worker(
                stream_num,
                start,
                end,
                &input_file,
                &remote_file,
                &cfg_clone,
                pb,
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

    // Wait for all threads
    for handle in handles {
        let _ = handle.join();
    }

    // Check for failures
    let flags = retry_flag.lock().unwrap();
    if flags.iter().any(|&flag| flag) {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "Some streams failed to transfer after retries."
        ));
    }

    println!("Transfer completed successfully!");

    // Print final statistics
    let stats = stats.lock().unwrap();
    print_transfer_stats(&stats, num_streams);

    Ok(())
}
