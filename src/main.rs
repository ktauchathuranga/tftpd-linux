use std::collections::HashMap;
use std::env;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::{UdpSocket, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

// TFTP Opcodes
const RRQ: u16 = 1;   // Read request
const WRQ: u16 = 2;   // Write request
const DATA: u16 = 3;  // Data packet
const ERROR: u16 = 5; // Error packet

// Error codes
const ERROR_FILE_NOT_FOUND: u16 = 1;
const ERROR_ACCESS_VIOLATION: u16 = 2;
const ERROR_ILLEGAL_OPERATION: u16 = 4;

#[derive(Debug)]
struct TFTPServer {
    port: u16,
    directory: PathBuf,
    active_transfers: Arc<Mutex<HashMap<String, bool>>>,
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

    fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        let socket = UdpSocket::bind(format!("0.0.0.0:{}", self.port))?;
        
        println!("🔧 Simple TFTP Server - Rust Edition");
        println!("====================================");
        println!("🚀 TFTP Server started on port {}", self.port);
        println!("📁 Serving files from: {}", self.directory.display());
        println!("🔗 Server IP: {}", self.get_local_ip());
        println!("📡 Waiting for requests... (Ctrl+C to stop)");
        println!("{}", "-".repeat(50));

        let mut buffer = [0; 1024];
        
        loop {
            match socket.recv_from(&mut buffer) {
                Ok((size, addr)) => {
                    let data = buffer[..size].to_vec();
                    let server_clone = self.clone();
                    
                    thread::spawn(move || {
                        if let Err(e) = server_clone.handle_request(&data, addr) {
                            eprintln!("❌ Error handling request from {}: {}", addr, e);
                        }
                    });
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::Interrupted {
                        break;
                    }
                    eprintln!("❌ Error receiving data: {}", e);
                }
            }
        }
        
        println!("\n👋 Server stopped.");
        Ok(())
    }

    fn handle_request(&self, data: &[u8], addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
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

    fn handle_read_request(&self, data: &[u8], addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
        let (filename, _mode) = self.parse_request(&data[2..])?;
        let filepath = self.directory.join(&filename);
        
        // Security check - prevent directory traversal
        if !filepath.starts_with(&self.directory) {
            println!("⚠️  Access violation attempt: {} from {}", filename, addr.ip());
            self.send_error(addr, ERROR_ACCESS_VIOLATION, "Access violation")?;
            return Ok(());
        }

        if !filepath.exists() || !filepath.is_file() {
            println!("❌ File not found: {} (requested by {})", filename, addr.ip());
            self.send_error(addr, ERROR_FILE_NOT_FOUND, "File not found")?;
            return Ok(());
        }

        let file_size = std::fs::metadata(&filepath)?.len();
        println!("📤 Upload started: {} ({}) to {}:{}", 
                filename, format_size(file_size), addr.ip(), addr.port());

        // Create new socket for this transfer
        let transfer_socket = UdpSocket::bind("0.0.0.0:0")?;
        transfer_socket.set_read_timeout(Some(Duration::from_secs(5)))?;
        
        self.send_file(&filepath, addr, &transfer_socket, &filename, file_size)?;
        
        Ok(())
    }

    fn handle_write_request(&self, data: &[u8], addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
        let (filename, _mode) = self.parse_request(&data[2..])?;
        let filepath = self.directory.join(&filename);
        
        // Security check
        if !filepath.starts_with(&self.directory) {
            self.send_error(addr, ERROR_ACCESS_VIOLATION, "Access violation")?;
            return Ok(());
        }

        if filepath.exists() {
            println!("⚠️  File exists, overwriting: {} (from {})", filename, addr.ip());
        } else {
            println!("📥 Download started: {} from {}:{}", filename, addr.ip(), addr.port());
        }

        // Create new socket for this transfer
        let transfer_socket = UdpSocket::bind("0.0.0.0:0")?;
        transfer_socket.set_read_timeout(Some(Duration::from_secs(10)))?;
        
        self.receive_file(&filepath, addr, &transfer_socket, &filename)?;
        
        Ok(())
    }

    fn send_file(&self, filepath: &Path, addr: SocketAddr, socket: &UdpSocket, filename: &str, file_size: u64) -> Result<(), Box<dyn std::error::Error>> {
        let mut file = File::open(filepath)?;
        let mut buffer = [0; 512];
        let mut block_num: u16 = 1;
        let mut bytes_sent = 0u64;
        let mut last_progress = -1i32;

        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }

            // Create DATA packet
            let mut packet = Vec::with_capacity(4 + bytes_read);
            packet.extend_from_slice(&DATA.to_be_bytes());
            packet.extend_from_slice(&block_num.to_be_bytes());
            packet.extend_from_slice(&buffer[..bytes_read]);

            // Send with retries
            let mut retries = 0;
            let mut acked = false;
            
            while retries < 5 && !acked {
                socket.send_to(&packet, addr)?;
                
                match socket.recv_from(&mut [0; 1024]) {
                    Ok((ack_size, _)) => {
                        if ack_size >= 4 {
                            // Simple ACK validation - in a full implementation, 
                            // we'd parse the ACK properly to check block number
                            acked = true;
                        }
                    }
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::TimedOut {
                            retries += 1;
                        } else {
                            return Err(e.into());
                        }
                    }
                }
            }

            if !acked {
                println!("❌ Transfer timeout: {}", filename);
                return Ok(());
            }

            bytes_sent += bytes_read as u64;
            let progress = if file_size > 0 { 
                ((bytes_sent * 100) / file_size) as i32 
            } else { 
                100 
            };

            // Show progress every 5%
            if progress != last_progress && progress % 5 == 0 {
                println!("📤 {}: {}% ({}/{})", 
                    filename, progress, 
                    format_size(bytes_sent), 
                    format_size(file_size));
                last_progress = progress;
            }

            block_num = block_num.wrapping_add(1);

            // If less than 512 bytes, this is the last packet
            if bytes_read < 512 {
                break;
            }
        }

        println!("✅ Upload completed: {} ({}) to {}", 
                filename, format_size(bytes_sent), addr.ip());
        Ok(())
    }

    fn receive_file(&self, filepath: &Path, addr: SocketAddr, socket: &UdpSocket, filename: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Send ACK 0 to start transfer
        let ack_packet = [0, 4, 0, 0]; // ACK opcode + block 0
        socket.send_to(&ack_packet, addr)?;

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(filepath)?;

        let mut expected_block: u16 = 1;
        let mut bytes_received = 0u64;
        let mut buffer = [0; 1024];

        loop {
            match socket.recv_from(&mut buffer) {
                Ok((size, _)) => {
                    if size < 4 {
                        continue;
                    }

                    let opcode = u16::from_be_bytes([buffer[0], buffer[1]]);
                    let block_num = u16::from_be_bytes([buffer[2], buffer[3]]);

                    if opcode == DATA && block_num == expected_block {
                        let file_data = &buffer[4..size];
                        file.write_all(file_data)?;
                        bytes_received += file_data.len() as u64;

                        // Send ACK
                        let ack_packet = [0, 4, buffer[2], buffer[3]]; // ACK + block number
                        socket.send_to(&ack_packet, addr)?;

                        println!("📥 {}: Block {} ({} received)", 
                                filename, block_num, format_size(bytes_received));

                        expected_block = expected_block.wrapping_add(1);

                        // Last packet (less than 512 bytes of data)
                        if file_data.len() < 512 {
                            break;
                        }
                    } else if opcode == DATA {
                        // Resend last ACK for duplicate or out-of-order packet
                        let prev_block = expected_block.wrapping_sub(1);
                        let ack_packet = [0, 4, (prev_block >> 8) as u8, prev_block as u8];
                        socket.send_to(&ack_packet, addr)?;
                    }
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::TimedOut {
                        println!("❌ Transfer timeout: {}", filename);
                        return Ok(());
                    }
                    return Err(e.into());
                }
            }
        }

        println!("✅ Download completed: {} ({}) from {}", 
                filename, format_size(bytes_received), addr.ip());
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

    fn send_error(&self, addr: SocketAddr, error_code: u16, error_msg: &str) -> Result<(), Box<dyn std::error::Error>> {
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
        // Simple way to get local IP - connect to a remote address
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
        println!("\n💡 Try these alternatives:");
        let alternative_ports = [6969, 6900, 7069, 8069, 9069];
        
        for &alt_port in &alternative_ports {
            if alt_port != self.port {
                if self.check_port_available(alt_port) {
                    println!("   ✅ Port {}: ./tftp_server {}", alt_port, alt_port);
                } else {
                    println!("   ❌ Port {}: (busy)", alt_port);
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

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut port = 6969u16; // Default non-privileged port
    
    if args.len() > 1 {
        match args[1].parse::<u16>() {
            Ok(p) => port = p,
            Err(_) => {
                eprintln!("❌ Invalid port number");
                std::process::exit(1);
            }
        }
    }
    
    // Check if we need root for ports < 1024
    if port < 1024 && unsafe { libc::geteuid() } != 0 {
        println!("⚠️  Port {} requires root privileges. Using port 6969 instead.", port);
        println!("💡 Run with sudo to use port 69, or specify a port > 1024");
        port = 6969;
    }
    
    let server = TFTPServer::new(port, None);
    
    match server.start() {
        Ok(_) => {},
        Err(e) => {
            if let Some(io_error) = e.downcast_ref::<std::io::Error>() {
                if io_error.kind() == std::io::ErrorKind::AddrInUse {
                    println!("❌ Port {} is already in use!", port);
                    server.suggest_alternative_ports();
                    std::process::exit(1);
                }
            }
            eprintln!("❌ Error starting server: {}", e);
            std::process::exit(1);
        }
    }
}
