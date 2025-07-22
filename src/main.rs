use std::collections::HashMap;
use std::env;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write, stderr, stdout};
use std::net::{SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

// TFTP Opcodes
const RRQ: u16 = 1; // Read request
const WRQ: u16 = 2; // Write request
const DATA: u16 = 3; // Data packet
const ACK: u16 = 4;  // Acknowledgment packet
const ERROR: u16 = 5; // Error packet

// Error codes
const ERROR_FILE_NOT_FOUND: u16 = 1;
const ERROR_ACCESS_VIOLATION: u16 = 2;
const ERROR_ILLEGAL_OPERATION: u16 = 4;

// WiFi-optimized constants
const MAX_RETRIES: usize = 8; // Increased for WiFi
const INITIAL_TIMEOUT_MS: u64 = 1000; // Start with 1 second
const MAX_TIMEOUT_MS: u64 = 5000; // Max 5 seconds
const PACKET_SIZE: usize = 512; // Standard TFTP packet size

#[derive(Debug)]
struct TFTPServer {
    port: u16,
    directory: PathBuf,
    active_transfers: Arc<Mutex<HashMap<String, bool>>>,
}

struct ProgressBar {
    filename: String,
    width: usize,
    terminal_width: usize,
}

impl ProgressBar {
    fn new(filename: String) -> Self {
        let terminal_width = get_terminal_width();
        // Calculate available width for the progress bar itself
        let reserved_space = 47 + filename.len();
        let available_for_bar = if terminal_width > reserved_space + 10 {
            std::cmp::min(40, terminal_width - reserved_space)
        } else {
            20 // Minimum bar width
        };

        Self {
            filename,
            width: available_for_bar,
            terminal_width,
        }
    }

    fn update(&mut self, progress: u32, bytes_transferred: u64, total_bytes: u64, speed: f64) {
        let filled = (progress * self.width as u32 / 100) as usize;
        let empty = self.width - filled;

        let bar = if filled == 0 {
            format!(">{}", "-".repeat(empty.saturating_sub(1)))
        } else if filled >= self.width {
            "=".repeat(self.width)
        } else {
            format!(
                "{}>{}",
                "=".repeat(filled.saturating_sub(1)),
                "-".repeat(empty.saturating_sub(1))
            )
        };

        let speed_str = if speed > 1024.0 * 1024.0 {
            format!("{:.1}MB/s", speed / (1024.0 * 1024.0))
        } else if speed > 1024.0 {
            format!("{:.1}KB/s", speed / 1024.0)
        } else {
            format!("{:.0}B/s", speed)
        };

        let display_filename = if self.filename.len() > 15 {
            format!("{}...", &self.filename[..12])
        } else {
            self.filename.clone()
        };

        let line = format!(
            "[{}] {}% ({}/{}) {} - {}",
            bar,
            progress,
            format_size_compact(bytes_transferred),
            format_size_compact(total_bytes),
            speed_str,
            display_filename
        );

        let final_line = if line.len() > self.terminal_width {
            format!("{}...", &line[..self.terminal_width.saturating_sub(3)])
        } else {
            line
        };

        eprint!("\r\x1B[K{}", final_line);
        let _ = stderr().flush();
    }

    fn finish(&mut self, operation: &str, bytes: u64, addr: std::net::IpAddr) {
        eprintln!();
        println!(
            "[INFO] {} completed: {} ({}) {}",
            operation,
            self.filename,
            format_size(bytes),
            if operation == "Upload" {
                format!("to {}", addr)
            } else {
                format!("from {}", addr)
            }
        );
    }

    fn error(&mut self, message: &str) {
        eprintln!();
        println!("[ERROR] {}: {}", self.filename, message);
    }

    fn retry_info(&mut self, retry: usize, max_retries: usize) {
        eprint!("\r\x1B[K[RETRY {}/{}] {} - Network timeout, retrying...", 
                retry, max_retries, self.filename);
        let _ = stderr().flush();
    }
}

fn get_terminal_width() -> usize {
    if let Some(width) = get_terminal_width_ioctl() {
        return width;
    }
    if let Some(width) = get_terminal_width_env() {
        return width;
    }
    if let Some(width) = get_terminal_width_stty() {
        return width;
    }
    80
}

fn get_terminal_width_ioctl() -> Option<usize> {
    use std::mem;

    #[repr(C)]
    struct WinSize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }

    let mut ws: WinSize = unsafe { mem::zeroed() };

    #[cfg(target_os = "linux")]
    const TIOCGWINSZ: libc::c_ulong = 0x5413;
    #[cfg(target_os = "macos")]
    const TIOCGWINSZ: libc::c_ulong = 0x40087468;
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    const TIOCGWINSZ: libc::c_ulong = 0x5413;

    unsafe {
        if libc::ioctl(libc::STDOUT_FILENO, TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 {
            return Some(ws.ws_col as usize);
        }
    }

    None
}

fn get_terminal_width_env() -> Option<usize> {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&w| w > 0)
}

fn get_terminal_width_stty() -> Option<usize> {
    use std::process::Command;

    let output = Command::new("stty").arg("size").output().ok()?;
    let size_str = String::from_utf8(output.stdout).ok()?;
    let parts: Vec<&str> = size_str.trim().split_whitespace().collect();

    if parts.len() >= 2 {
        parts[1].parse::<usize>().ok().filter(|&w| w > 0)
    } else {
        None
    }
}

// Adaptive timeout calculation
fn calculate_timeout(retry: usize) -> Duration {
    let timeout_ms = std::cmp::min(
        INITIAL_TIMEOUT_MS * (2_u64.pow(retry as u32)),
        MAX_TIMEOUT_MS
    );
    Duration::from_millis(timeout_ms)
}

impl TFTPServer {
    fn new(port: u16, directory: Option<PathBuf>) -> Self {
        let dir = directory.unwrap_or_else(|| env::current_dir().unwrap());
        TFTPServer {
            port,
            directory: dir,
            active_transfers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn clear_terminal(&self) {
        print!("\x1B[2J\x1B[1;1H");
        let _ = stdout().flush();
    }

    fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.clear_terminal();

        let socket = UdpSocket::bind(format!("0.0.0.0:{}", self.port))?;

        // Optimize socket for better WiFi performance
        self.optimize_socket(&socket)?;

        println!(" _    __ _             _        _ _                  ");
        println!("| |  / _| |           | |      | (_)                 ");
        println!("| |_| |_| |_ _ __   __| |______| |_ _ __  _   ___  __");
        println!("| __|  _| __| '_ \\ / _` |______| | | '_ \\| | | \\ \\/ /");
        println!("| |_| | | |_| |_) | (_| |      | | | | | | |_| |>  < ");
        println!(" \\__|_|  \\__| .__/ \\__,_|      |_|_|_| |_|\\__,_/_/\\_\\");
        println!("            | |                                      ");
        println!("            |_|                                      ");
        println!("{}", "=".repeat(53));
        println!("[-] TFTP Server started on port {}", self.port);
        println!("[-] Serving files from: {}", self.directory.display());
        println!("[-] Server IP: {}", self.get_local_ip());
        println!("[-] Waiting for requests... (Ctrl+C to stop)");
        println!("{}", "-".repeat(53));

        let mut buffer = [0; 1024];

        loop {
            match socket.recv_from(&mut buffer) {
                Ok((size, addr)) => {
                    let data = buffer[..size].to_vec();
                    let server_clone = self.clone();

                    thread::spawn(move || {
                        if let Err(e) = server_clone.handle_request(&data, addr) {
                            eprintln!("[ERROR] Error handling request from {}: {}", addr, e);
                        }
                    });
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::Interrupted {
                        break;
                    }
                    eprintln!("[ERROR] Error receiving data: {}", e);
                    thread::sleep(Duration::from_millis(100)); // Brief pause on error
                }
            }
        }

        println!("\n[INFO] Server stopped.");
        Ok(())
    }

    fn optimize_socket(&self, socket: &UdpSocket) -> Result<(), Box<dyn std::error::Error>> {
        // Increase socket buffer sizes for better WiFi performance
        const BUFFER_SIZE: usize = 256 * 1024; // 256KB

        use std::os::unix::io::AsRawFd;
        let fd = socket.as_raw_fd();

        unsafe {
            // Set receive buffer size
            let optval = BUFFER_SIZE as libc::c_int;
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_RCVBUF,
                &optval as *const libc::c_int as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            );

            // Set send buffer size
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                &optval as *const libc::c_int as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            );
        }

        Ok(())
    }

    fn handle_request(
        &self,
        data: &[u8],
        addr: SocketAddr,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if data.len() < 2 {
            return Err("Invalid packet size".into());
        }

        let opcode = u16::from_be_bytes([data[0], data[1]]);

        match opcode {
            RRQ => self.handle_read_request(data, addr),
            WRQ => self.handle_write_request(data, addr),
            _ => {
                self.send_error(addr, ERROR_ILLEGAL_OPERATION, "Illegal TFTP operation")?;
                Ok(())
            }
        }
    }

    fn handle_read_request(
        &self,
        data: &[u8],
        addr: SocketAddr,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (filename, _mode) = self.parse_request(&data[2..])?;
        let filepath = self.directory.join(&filename);

        if !filepath.starts_with(&self.directory) {
            println!(
                "[INFO] Access violation attempt: {} from {}",
                filename,
                addr.ip()
            );
            self.send_error(addr, ERROR_ACCESS_VIOLATION, "Access violation")?;
            return Ok(());
        }

        if !filepath.exists() || !filepath.is_file() {
            println!(
                "[ERROR] File not found: {} (requested by {})",
                filename,
                addr.ip()
            );
            self.send_error(addr, ERROR_FILE_NOT_FOUND, "File not found")?;
            return Ok(());
        }

        let file_size = std::fs::metadata(&filepath)?.len();
        println!(
            "[INFO] Upload started: {} ({}) to {}:{}",
            filename,
            format_size(file_size),
            addr.ip(),
            addr.port()
        );

        let transfer_socket = UdpSocket::bind("0.0.0.0:0")?;
        self.optimize_socket(&transfer_socket)?;

        self.send_file(&filepath, addr, &transfer_socket, &filename, file_size)?;
        Ok(())
    }

    fn handle_write_request(
        &self,
        data: &[u8],
        addr: SocketAddr,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (filename, _mode) = self.parse_request(&data[2..])?;
        let filepath = self.directory.join(&filename);

        if !filepath.starts_with(&self.directory) {
            self.send_error(addr, ERROR_ACCESS_VIOLATION, "Access violation")?;
            return Ok(());
        }

        if filepath.exists() {
            println!(
                "[INFO] File exists, overwriting: {} (from {})",
                filename,
                addr.ip()
            );
        } else {
            println!(
                "[INFO] Download started: {} from {}:{}",
                filename,
                addr.ip(),
                addr.port()
            );
        }

        let transfer_socket = UdpSocket::bind("0.0.0.0:0")?;
        self.optimize_socket(&transfer_socket)?;

        self.receive_file(&filepath, addr, &transfer_socket, &filename)?;
        Ok(())
    }

    fn send_file(
        &self,
        filepath: &Path,
        addr: SocketAddr,
        socket: &UdpSocket,
        filename: &str,
        file_size: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut file = File::open(filepath)?;
        let mut buffer = [0; PACKET_SIZE];
        let mut block_num: u16 = 1;
        let mut bytes_sent = 0u64;
        let mut progress_bar = ProgressBar::new(filename.to_string());

        let start_time = Instant::now();
        let mut last_update = Instant::now();
        let mut consecutive_timeouts = 0;

        loop {
            let bytes_read = file.read(&mut buffer)?;

            let mut packet = Vec::with_capacity(4 + bytes_read);
            packet.extend_from_slice(&DATA.to_be_bytes());
            packet.extend_from_slice(&block_num.to_be_bytes());
            packet.extend_from_slice(&buffer[..bytes_read]);

            let mut retries = 0;
            let mut acked = false;
            let mut ack_buffer = [0; 1024];

            while retries < MAX_RETRIES && !acked {
                socket.send_to(&packet, addr)?;

                let timeout = calculate_timeout(retries);
                socket.set_read_timeout(Some(timeout))?;

                match socket.recv_from(&mut ack_buffer) {
                    Ok((ack_size, recv_addr)) => {
                        if recv_addr == addr && ack_size >= 4 {
                            let ack_opcode = u16::from_be_bytes([ack_buffer[0], ack_buffer[1]]);
                            let ack_block = u16::from_be_bytes([ack_buffer[2], ack_buffer[3]]);
                            
                            if ack_opcode == ACK && ack_block == block_num {
                                acked = true;
                                consecutive_timeouts = 0; // Reset timeout counter
                            } else if ack_opcode == ACK && ack_block == block_num.wrapping_sub(1) {
                                // Duplicate ACK, just continue
                                continue;
                            } else {
                                retries += 1;
                            }
                        }
                    }
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::TimedOut 
                           || e.kind() == std::io::ErrorKind::WouldBlock {
                            retries += 1;
                            consecutive_timeouts += 1;
                            
                            if retries < MAX_RETRIES {
                                progress_bar.retry_info(retries, MAX_RETRIES);
                                
                                // Add small delay for WiFi stability
                                thread::sleep(Duration::from_millis(50 + (retries * 25) as u64));
                            }
                        } else {
                            return Err(format!("Network error: {}", e).into());
                        }
                    }
                }
            }

            if !acked {
                progress_bar.error(&format!("Transfer failed after {} retries - network unstable", MAX_RETRIES));
                return Ok(());
            }

            // Adaptive delay based on network conditions
            if consecutive_timeouts > 3 {
                thread::sleep(Duration::from_millis(100)); // Slow down on poor network
            }

            bytes_sent += bytes_read as u64;
            let now = Instant::now();

            if now.duration_since(last_update).as_millis() >= 100 || bytes_read < PACKET_SIZE {
                let progress = if file_size > 0 {
                    ((bytes_sent * 100) / file_size) as u32
                } else {
                    100
                };

                let elapsed = now.duration_since(start_time).as_secs_f64();
                let speed = if elapsed > 0.0 {
                    bytes_sent as f64 / elapsed
                } else {
                    0.0
                };

                progress_bar.update(progress, bytes_sent, file_size, speed);
                last_update = now;
            }

            block_num = block_num.wrapping_add(1);

            if bytes_read < PACKET_SIZE {
                break;
            }
        }

        progress_bar.finish("Upload", bytes_sent, addr.ip());
        Ok(())
    }

    fn receive_file(
        &self,
        filepath: &Path,
        addr: SocketAddr,
        socket: &UdpSocket,
        filename: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let ack_packet = [0, 4, 0, 0];
        socket.send_to(&ack_packet, addr)?;

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(filepath)?;

        let mut expected_block: u16 = 1;
        let mut bytes_received = 0u64;
        let mut buffer = [0; 1024];
        let mut progress_bar = ProgressBar::new(filename.to_string());
        let mut consecutive_timeouts = 0;

        let start_time = Instant::now();
        let mut last_update = Instant::now();
        let mut last_progress = 0u32;

        socket.set_read_timeout(Some(Duration::from_millis(INITIAL_TIMEOUT_MS * 2)))?;

        loop {
            match socket.recv_from(&mut buffer) {
                Ok((size, recv_addr)) => {
                    if recv_addr != addr || size < 4 {
                        continue;
                    }

                    consecutive_timeouts = 0; // Reset on successful receive

                    let opcode = u16::from_be_bytes([buffer[0], buffer[1]]);
                    let block_num = u16::from_be_bytes([buffer[2], buffer[3]]);

                    if opcode == DATA && block_num == expected_block {
                        let file_data = &buffer[4..size];
                        file.write_all(file_data)?;
                        bytes_received += file_data.len() as u64;

                        let ack_packet = [0, 4, buffer[2], buffer[3]];
                        socket.send_to(&ack_packet, addr)?;

                        let now = Instant::now();
                        let is_last_packet = file_data.len() < PACKET_SIZE;

                        let progress = if is_last_packet {
                            100
                        } else {
                            let mb_received = bytes_received / (1024 * 1024);
                            std::cmp::min((mb_received * 2).min(95) as u32, 95)
                        };

                        let should_update = now.duration_since(last_update).as_millis() >= 100
                            || progress != last_progress
                            || is_last_packet;

                        if should_update {
                            let elapsed = now.duration_since(start_time).as_secs_f64();
                            let speed = if elapsed > 0.0 {
                                bytes_received as f64 / elapsed
                            } else {
                                0.0
                            };

                            progress_bar.update(progress, bytes_received, bytes_received, speed);
                            last_update = now;
                            last_progress = progress;
                        }

                        expected_block = expected_block.wrapping_add(1);

                        if is_last_packet {
                            break;
                        }
                    } else if opcode == DATA {
                        if block_num == expected_block.wrapping_sub(1) {
                            let prev_block = expected_block.wrapping_sub(1);
                            let ack_packet = [0, 4, (prev_block >> 8) as u8, prev_block as u8];
                            socket.send_to(&ack_packet, addr)?;
                        }
                    } else if opcode == ERROR {
                        let error_code = u16::from_be_bytes([buffer[2], buffer[3]]);
                        let error_msg = String::from_utf8_lossy(&buffer[4..size-1]);
                        progress_bar.error(&format!("Client error {}: {}", error_code, error_msg));
                        return Ok(());
                    }
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::TimedOut 
                       || e.kind() == std::io::ErrorKind::WouldBlock {
                        consecutive_timeouts += 1;
                        
                        if consecutive_timeouts >= MAX_RETRIES {
                            progress_bar.error("Transfer timeout - network unstable");
                            return Ok(());
                        }
                        
                        // Increase timeout on consecutive failures
                        let new_timeout = calculate_timeout(consecutive_timeouts);
                        socket.set_read_timeout(Some(new_timeout))?;
                        
                        continue;
                    }
                    return Err(format!("Network error: {}", e).into());
                }
            }
        }

        progress_bar.finish("Download", bytes_received, addr.ip());
        Ok(())
    }

    fn parse_request(&self, data: &[u8]) -> Result<(String, String), Box<dyn std::error::Error>> {
        let mut parts = Vec::new();
        let mut current = Vec::new();

        for &byte in data {
            if byte == 0 {
                if !current.is_empty() {
                    parts.push(String::from_utf8(current)?);
                    current = Vec::new();
                }
                if parts.len() >= 2 {
                    break;
                }
            } else {
                current.push(byte);
            }
        }

        if parts.len() < 2 {
            return Err("Malformed request".into());
        }

        Ok((parts[0].clone(), parts[1].clone()))
    }

    fn send_error(
        &self,
        addr: SocketAddr,
        error_code: u16,
        error_msg: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;

        let mut packet = Vec::new();
        packet.extend_from_slice(&ERROR.to_be_bytes());
        packet.extend_from_slice(&error_code.to_be_bytes());
        packet.extend_from_slice(error_msg.as_bytes());
        packet.push(0);

        socket.send_to(&packet, addr)?;
        Ok(())
    }

    fn get_local_ip(&self) -> String {
        match UdpSocket::bind("0.0.0.0:0") {
            Ok(socket) => {
                if let Ok(_) = socket.connect("8.8.8.8:80") {
                    if let Ok(addr) = socket.local_addr() {
                        return addr.ip().to_string();
                    }
                }
            }
            Err(_) => {}
        }
        "127.0.0.1".to_string()
    }

    fn check_port_available(&self, port: u16) -> bool {
        UdpSocket::bind(format!("0.0.0.0:{}", port)).is_ok()
    }

    fn suggest_alternative_ports(&self) {
        println!("\n[INFO] Try these alternatives:");
        let alternative_ports = [6969, 6900, 7069, 8069, 9069];

        for &alt_port in &alternative_ports {
            if alt_port != self.port {
                if self.check_port_available(alt_port) {
                    println!("   [INFO] Port {}: ./tftp_server {}", alt_port, alt_port);
                } else {
                    println!("   [ERROR] Port {}: (busy)", alt_port);
                }
            }
        }
    }
}

impl Clone for TFTPServer {
    fn clone(&self) -> Self {
        TFTPServer {
            port: self.port,
            directory: self.directory.clone(),
            active_transfers: Arc::clone(&self.active_transfers),
        }
    }
}

fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    format!("{:.1} {}", size, UNITS[unit_index])
}

fn format_size_compact(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    format!("{:.1}{}", size, UNITS[unit_index])
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut port = 6969u16;

    if args.len() > 1 {
        match args[1].parse::<u16>() {
            Ok(p) => port = p,
            Err(_) => {
                eprintln!("[ERROR] Invalid port number");
                std::process::exit(1);
            }
        }
    }

    if port < 1024 && unsafe { libc::geteuid() } != 0 {
        println!(
            "[INFO] Port {} requires root privileges. Using port 6969 instead.",
            port
        );
        println!("[INFO] Run with sudo to use port 69, or specify a port > 1024");
        port = 6969;
    }

    let server = TFTPServer::new(port, None);

    match server.start() {
        Ok(_) => {}
        Err(e) => {
            if let Some(io_error) = e.downcast_ref::<std::io::Error>() {
                if io_error.kind() == std::io::ErrorKind::AddrInUse {
                    println!("[ERROR] Port {} is already in use!", port);
                    server.suggest_alternative_ports();
                    std::process::exit(1);
                }
            }
            eprintln!("[ERROR] Error starting server: {}", e);
            std::process::exit(1);
        }
    }
}
