use std::fs::File;
use std::io::{self, Read, Write, Seek, SeekFrom};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use std::env;
use ssh2::{Session, Sftp, OpenFlags, OpenType};
use indicatif::ProgressBar;

const BUFFER_SIZE: usize = 1 * 1024 * 1024; // 1MB
const CONNECTION_TIMEOUT_SECS: u64 = 30;
const BASE_RETRY_DELAY_MS: u64 = 1000;
const MAX_RETRY_DELAY_MS: u64 = 30000;

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub key_path: Option<String>,
    pub retries: u32,
}

/// Connect to SSH server and authenticate
pub fn connect_and_auth(cfg: &SessionConfig) -> io::Result<Session> {
    let addr = format!("{}:{}", cfg.host, cfg.port);

    // Resolve hostname to socket addresses (handles DNS and mDNS)
    let socket_addrs: Vec<_> = addr
        .to_socket_addrs()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("Failed to resolve host {}: {}", cfg.host, e)))?
        .collect();

    let socket_addr = socket_addrs.first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("Could not resolve hostname: {}", cfg.host)))?;

    // TCP connect with timeout
    let tcp = TcpStream::connect_timeout(socket_addr, Duration::from_secs(CONNECTION_TIMEOUT_SECS))?;
    tcp.set_nodelay(true)?;

    // SSH handshake
    let mut sess = Session::new()?;
    sess.set_tcp_stream(tcp);
    sess.handshake()?;

    // Authentication: try key file, then default keys, then agent

    // 1. Try explicit key path if provided
    if let Some(ref key_path) = cfg.key_path {
        if let Err(e) = sess.userauth_pubkey_file(&cfg.user, None, Path::new(key_path), None) {
            eprintln!("Warning: Specified key auth failed: {}. Trying defaults...", e);
        }
    }

    // 2. Try default SSH key locations
    if !sess.authenticated() {
        let home = env::var("HOME").ok();
        let default_keys = if let Some(ref home_dir) = home {
            vec![
                PathBuf::from(home_dir).join(".ssh/id_ed25519"),
                PathBuf::from(home_dir).join(".ssh/id_rsa"),
                PathBuf::from(home_dir).join(".ssh/id_ecdsa"),
            ]
        } else {
            vec![]
        };

        for key_path in default_keys {
            if key_path.exists() {
                if sess.userauth_pubkey_file(&cfg.user, None, &key_path, None).is_ok() {
                    break;
                }
            }
        }
    }

    // 3. Fall back to SSH agent
    if !sess.authenticated() {
        let _ = sess.userauth_agent(&cfg.user);
    }

    if !sess.authenticated() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Failed to authenticate with SSH server. Try specifying a key with --ssh-key-path",
        ));
    }

    Ok(sess)
}

/// Open SFTP channel
pub fn open_sftp(sess: &Session) -> io::Result<Sftp> {
    sess.sftp().map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}

/// Get remote file size via SFTP stat
pub fn stat_remote_file(sftp: &Sftp, path: &str) -> io::Result<u64> {
    let stat = sftp.stat(Path::new(path))
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to stat remote file: {}", e)))?;

    stat.size.ok_or_else(|| io::Error::new(
        io::ErrorKind::Other,
        "Remote file stat did not return size",
    ))
}

/// Extend remote file to specified size (sparse allocation)
pub fn extend_remote_file(sftp: &Sftp, path: &str, size: u64) -> io::Result<()> {
    // Open file with CREATE | WRITE | TRUNCATE
    let mut file = sftp.open_mode(
        Path::new(path),
        OpenFlags::CREATE | OpenFlags::WRITE | OpenFlags::TRUNCATE,
        0o644,
        OpenType::File,
    ).map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to open remote file: {}", e)))?;

    // Extend file by seeking to size-1 and writing a single byte
    // This creates a sparse file on most filesystems
    if size > 0 {
        file.seek(SeekFrom::Start(size.saturating_sub(1)))?;
        file.write_all(&[0])?;
    }

    Ok(())
}

/// Cross-platform positional write for local files
#[cfg(unix)]
pub fn write_at_local(file: &File, buf: &[u8], offset: u64) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;
    file.write_at(buf, offset)
}

#[cfg(windows)]
pub fn write_at_local(file: &File, buf: &[u8], offset: u64) -> io::Result<usize> {
    use std::os::windows::fs::FileExt;
    file.seek_write(buf, offset)
}

/// Calculate retry delay with exponential backoff and jitter
fn calculate_retry_delay(attempt: u32) -> Duration {
    let delay_ms = std::cmp::min(
        BASE_RETRY_DELAY_MS * 2_u64.pow(attempt),
        MAX_RETRY_DELAY_MS,
    );

    // Add ±20% jitter
    let jitter = (delay_ms as f64 * 0.2 * (rand::random::<f64>() - 0.5)) as i64;
    let final_delay = (delay_ms as i64 + jitter).max(0) as u64;

    Duration::from_millis(final_delay)
}

/// Pull worker: stream data from remote to local using SFTP
pub fn pull_worker(
    stream_num: usize,
    start: u64,
    end: u64,
    remote_file: &str,
    cfg: &SessionConfig,
    local_file: &File,
    pb: ProgressBar,
) -> io::Result<()> {
    let bytes_to_read = (end - start) as usize;
    let mut attempt = 0;

    while attempt <= cfg.retries {
        let result = (|| -> io::Result<()> {
            // Create new session for this stream
            let sess = connect_and_auth(cfg)?;
            let sftp = open_sftp(&sess)?;

            // Open remote file
            let mut remote = sftp.open(Path::new(remote_file))
                .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to open remote file: {}", e)))?;

            // Seek to start position
            remote.seek(SeekFrom::Start(start))?;

            // Read and write loop
            let mut buffer = vec![0u8; BUFFER_SIZE];
            let mut total_read = 0;
            let start_time = std::time::Instant::now();
            let mut last_update = start_time;

            while total_read < bytes_to_read {
                let to_read = std::cmp::min(BUFFER_SIZE, bytes_to_read - total_read);
                let n = remote.read(&mut buffer[..to_read])?;

                if n == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "Unexpected EOF from remote file",
                    ));
                }

                // Positional write to local file
                let offset = start + total_read as u64;
                let mut written = 0;
                while written < n {
                    let w = write_at_local(local_file, &buffer[written..n], offset + written as u64)?;
                    written += w;
                }

                total_read += n;
                pb.set_position(total_read as u64);

                // Update throughput display
                let now = std::time::Instant::now();
                if now.duration_since(last_update) > Duration::from_secs(1) {
                    let elapsed = now.duration_since(start_time).as_secs_f64();
                    let throughput = (total_read as f64 / 1024.0 / 1024.0) / elapsed;
                    pb.set_message(format!("{:.2} MB/s", throughput));
                    last_update = now;
                }
            }

            pb.finish_with_message("done");
            Ok(())
        })();

        match result {
            Ok(_) => return Ok(()),
            Err(e) => {
                attempt += 1;
                if attempt > cfg.retries {
                    pb.finish_with_message("failed");
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("Stream {} failed after {} retries: {}", stream_num, cfg.retries, e),
                    ));
                }

                let delay = calculate_retry_delay(attempt - 1);
                thread::sleep(delay);
            }
        }
    }

    Err(io::Error::new(
        io::ErrorKind::Other,
        format!("Stream {} failed after {} retries", stream_num, cfg.retries),
    ))
}

/// Push worker: stream data from local to remote using SFTP
pub fn push_worker(
    stream_num: usize,
    start: u64,
    end: u64,
    local_file_path: &str,
    remote_file: &str,
    cfg: &SessionConfig,
    pb: ProgressBar,
) -> io::Result<()> {
    let bytes_to_write = (end - start) as usize;
    let mut attempt = 0;

    while attempt <= cfg.retries {
        let result = (|| -> io::Result<()> {
            // Create new session for this stream
            let sess = connect_and_auth(cfg)?;
            let sftp = open_sftp(&sess)?;

            // Open remote file with WRITE flag (file should already exist and be extended)
            let mut remote = sftp.open_mode(
                Path::new(remote_file),
                OpenFlags::WRITE,
                0o644,
                OpenType::File,
            ).map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to open remote file: {}", e)))?;

            // Seek to start position
            remote.seek(SeekFrom::Start(start))?;

            // Open local file
            let mut local = File::open(local_file_path)?;
            local.seek(SeekFrom::Start(start))?;

            // Read and write loop
            let mut buffer = vec![0u8; BUFFER_SIZE];
            let mut total_written = 0;
            let start_time = std::time::Instant::now();
            let mut last_update = start_time;

            while total_written < bytes_to_write {
                let to_read = std::cmp::min(BUFFER_SIZE, bytes_to_write - total_written);
                let n = local.read(&mut buffer[..to_read])?;

                if n == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "Unexpected EOF from local file",
                    ));
                }

                // Write to remote file
                let mut written = 0;
                while written < n {
                    let w = remote.write(&buffer[written..n])?;
                    written += w;
                }

                total_written += n;
                pb.set_position(total_written as u64);

                // Update throughput display
                let now = std::time::Instant::now();
                if now.duration_since(last_update) > Duration::from_secs(1) {
                    let elapsed = now.duration_since(start_time).as_secs_f64();
                    let throughput = (total_written as f64 / 1024.0 / 1024.0) / elapsed;
                    pb.set_message(format!("{:.2} MB/s", throughput));
                    last_update = now;
                }
            }

            pb.finish_with_message("done");
            Ok(())
        })();

        match result {
            Ok(_) => return Ok(()),
            Err(e) => {
                attempt += 1;
                if attempt > cfg.retries {
                    pb.finish_with_message("failed");
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("Stream {} failed after {} retries: {}", stream_num, cfg.retries, e),
                    ));
                }

                let delay = calculate_retry_delay(attempt - 1);
                thread::sleep(delay);
            }
        }
    }

    Err(io::Error::new(
        io::ErrorKind::Other,
        format!("Stream {} failed after {} retries", stream_num, cfg.retries),
    ))
}
