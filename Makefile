# TFTP Server Makefile
# Simple TFTP server for Linux systems

# Variables
BINARY_NAME = tftpd-linux
PREFIX ?= /usr/local
BINDIR = $(PREFIX)/bin
MANDIR = $(PREFIX)/share/man/man1
TARGET_DIR = target/release
SOURCE_BINARY = $(TARGET_DIR)/$(BINARY_NAME)

# Colors for pretty output
GREEN = \033[0;32m
BLUE = \033[0;34m
YELLOW = \033[1;33m
RED = \033[0;31m
NC = \033[0m # No Color

# Default target
.PHONY: all
all: build

# Help target
.PHONY: help
help:
	@echo "$(BLUE)TFTP Server Build System$(NC)"
	@echo "========================="
	@echo ""
	@echo "$(GREEN)Available targets:$(NC)"
	@echo "  $(YELLOW)build$(NC)     - Build the release binary"
	@echo "  $(YELLOW)debug$(NC)     - Build the debug binary"
	@echo "  $(YELLOW)install$(NC)   - Install to system (requires sudo)"
	@echo "  $(YELLOW)uninstall$(NC) - Remove from system (requires sudo)"
	@echo "  $(YELLOW)clean$(NC)     - Clean build artifacts"
	@echo "  $(YELLOW)test$(NC)      - Run tests"
	@echo "  $(YELLOW)check$(NC)     - Check installation"
	@echo "  $(YELLOW)help$(NC)      - Show this help"
	@echo ""
	@echo "$(GREEN)Installation paths:$(NC)"
	@echo "  Binary: $(BINDIR)/$(BINARY_NAME)"
	@echo "  Manual: $(MANDIR)/$(BINARY_NAME).1"
	@echo ""
	@echo "$(GREEN)Usage after install:$(NC)"
	@echo "  $(YELLOW)tftpd-linux$(NC)          - Start server on port 6969"
	@echo "  $(YELLOW)tftpd-linux 69$(NC)       - Start server on port 69 (needs sudo)"
	@echo "  $(YELLOW)sudo tftpd-linux 69$(NC)  - Start server on privileged port"

# Check if Rust is installed
.PHONY: check-rust
check-rust:
	@which cargo >/dev/null 2>&1 || { \
		echo "$(RED)Error: Rust/Cargo not found!$(NC)"; \
		echo "Install Rust from: https://rustup.rs/"; \
		exit 1; \
	}

# Build release binary
.PHONY: build
build: check-rust
	@echo "$(BLUE)Building release binary...$(NC)"
	cargo build --release
	@echo "$(GREEN)✓ Build completed: $(SOURCE_BINARY)$(NC)"

# Build debug binary
.PHONY: debug
debug: check-rust
	@echo "$(BLUE)Building debug binary...$(NC)"
	cargo build
	@echo "$(GREEN)✓ Debug build completed: target/debug/$(BINARY_NAME)$(NC)"

# Install to system
.PHONY: install
install: build
	@echo "$(BLUE)Installing $(BINARY_NAME)...$(NC)"
	@# Check if we have write permissions
	@if [ ! -w "$(dir $(BINDIR))" ]; then \
		echo "$(RED)Error: No write permission to $(BINDIR)$(NC)"; \
		echo "$(YELLOW)Try: sudo make install$(NC)"; \
		exit 1; \
	fi
	@# Create directories if they don't exist
	install -d "$(BINDIR)"
	install -d "$(MANDIR)"
	@# Install binary
	install -m 755 "$(SOURCE_BINARY)" "$(BINDIR)/$(BINARY_NAME)"
	@# Create and install man page
	@echo "$(BLUE)Creating manual page...$(NC)"
	@$(MAKE) create-manpage
	install -m 644 $(BINARY_NAME).1 "$(MANDIR)/$(BINARY_NAME).1"
	@# Update man database
	@if command -v mandb >/dev/null 2>&1; then \
		echo "$(BLUE)Updating manual database...$(NC)"; \
		mandb -q 2>/dev/null || true; \
	fi
	@echo "$(GREEN)✓ Installation completed!$(NC)"
	@echo ""
	@echo "$(GREEN)You can now use:$(NC)"
	@echo "  $(YELLOW)tftpd-linux$(NC)      - Start server"
	@echo "  $(YELLOW)man tftpd-linux$(NC)  - Read manual"
	@echo ""
	@echo "$(BLUE)Example usage:$(NC)"
	@echo "  cd /path/to/files && tftpd-linux"
	@echo "  cd /path/to/files && sudo tftpd-linux 69"

# Uninstall from system
.PHONY: uninstall
uninstall:
	@echo "$(BLUE)Uninstalling $(BINARY_NAME)...$(NC)"
	@# Check if installed
	@if [ ! -f "$(BINDIR)/$(BINARY_NAME)" ]; then \
		echo "$(YELLOW)$(BINARY_NAME) is not installed$(NC)"; \
		exit 0; \
	fi
	@# Check permissions
	@if [ ! -w "$(BINDIR)" ]; then \
		echo "$(RED)Error: No write permission to $(BINDIR)$(NC)"; \
		echo "$(YELLOW)Try: sudo make uninstall$(NC)"; \
		exit 1; \
	fi
	@# Remove files
	rm -f "$(BINDIR)/$(BINARY_NAME)"
	rm -f "$(MANDIR)/$(BINARY_NAME).1"
	@# Update man database
	@if command -v mandb >/dev/null 2>&1; then \
		echo "$(BLUE)Updating manual database...$(NC)"; \
		mandb -q 2>/dev/null || true; \
	fi
	@echo "$(GREEN)✓ Uninstallation completed!$(NC)"

# Check installation
.PHONY: check
check:
	@echo "$(BLUE)Checking installation...$(NC)"
	@if [ -f "$(BINDIR)/$(BINARY_NAME)" ]; then \
		echo "$(GREEN)✓ Binary installed: $(BINDIR)/$(BINARY_NAME)$(NC)"; \
		ls -la "$(BINDIR)/$(BINARY_NAME)"; \
	else \
		echo "$(RED)✗ Binary not installed$(NC)"; \
	fi
	@if [ -f "$(MANDIR)/$(BINARY_NAME).1" ]; then \
		echo "$(GREEN)✓ Manual installed: $(MANDIR)/$(BINARY_NAME).1$(NC)"; \
	else \
		echo "$(YELLOW)⚠ Manual not installed$(NC)"; \
	fi
	@echo ""
	@echo "$(BLUE)Testing binary:$(NC)"
	@if command -v $(BINARY_NAME) >/dev/null 2>&1; then \
		echo "$(GREEN)✓ $(BINARY_NAME) is available in PATH$(NC)"; \
		echo "Version info:"; \
		$(BINARY_NAME) --help 2>/dev/null || echo "Binary exists but --help not implemented"; \
	else \
		echo "$(RED)✗ $(BINARY_NAME) not found in PATH$(NC)"; \
	fi

# Run tests
.PHONY: test
test: check-rust
	@echo "$(BLUE)Running tests...$(NC)"
	cargo test
	@echo "$(GREEN)✓ Tests completed$(NC)"

# Clean build artifacts
.PHONY: clean
clean:
	@echo "$(BLUE)Cleaning build artifacts...$(NC)"
	cargo clean
	rm -f $(BINARY_NAME).1
	@echo "$(GREEN)✓ Clean completed$(NC)"

# Create man page
.PHONY: create-manpage
create-manpage:
	@cat > $(BINARY_NAME).1 << 'EOF'
.TH TFTPD-LINUX 1 "$(shell date '+%B %Y')" "tftpd-linux 1.0" "User Commands"
.SH NAME
tftpd-linux \- Simple TFTP server for Linux systems
.SH SYNOPSIS
.B tftpd-linux
[\fIPORT\fR]
.SH DESCRIPTION
.B tftpd-linux
is a simple TFTP (Trivial File Transfer Protocol) server that serves files from the current working directory. It's designed to be similar to tftpd64 but runs natively on Linux systems.

The server supports both reading (downloading) and writing (uploading) files. It provides real-time progress tracking and handles multiple concurrent clients.

.SH OPTIONS
.TP
\fIPORT\fR
TCP port number to listen on. Default is 6969 for non-privileged operation. Port 69 is the standard TFTP port but requires root privileges.

.SH EXAMPLES
.TP
Start server on default port 6969:
.B cd /path/to/files && tftpd-linux

.TP
Start server on standard TFTP port 69 (requires root):
.B cd /path/to/files && sudo tftpd-linux 69

.TP
Start server on custom port:
.B cd /path/to/files && tftpd-linux 8069

.SH USAGE
.IP 1. 4
Navigate to the directory containing files you want to serve
.IP 2. 4
Start the server with: \fBtftpd-linux [port]\fR
.IP 3. 4
Clients can connect using any TFTP client:
   \fBtftp server_ip port\fR

.SH FEATURES
.IP \[bu] 2
Serves files from current working directory
.IP \[bu] 2
Real-time progress tracking for file transfers
.IP \[bu] 2
Support for multiple concurrent clients
.IP \[bu] 2
Security: Prevents directory traversal attacks
.IP \[bu] 2
Both upload and download support
.IP \[bu] 2
Human-readable file size display
.IP \[bu] 2
Automatic port conflict detection

.SH SECURITY
The server implements basic security measures:
.IP \[bu] 2
Files are only served from the current directory and subdirectories
.IP \[bu] 2
Directory traversal attempts (../) are blocked
.IP \[bu] 2
No authentication - suitable for trusted networks only

.SH FILES
The server serves files from the current working directory where it was started.

.SH EXIT STATUS
.TP
.B 0
Success
.TP
.B 1
Error (port in use, permission denied, etc.)

.SH AUTHOR
TFTP Server for Linux Systems

.SH SEE ALSO
.BR tftp (1),
.BR tftpd (8)

.SH BUGS
Report bugs to: https://github.com/your-repo/tftpd-linux
EOF

# Package for distribution
.PHONY: package
package: build
	@echo "$(BLUE)Creating distribution package...$(NC)"
	@VERSION=$$(cargo pkgid | cut -d'#' -f2); \
	PACKAGE_NAME="$(BINARY_NAME)-$$VERSION-x86_64-linux"; \
	mkdir -p "dist/$$PACKAGE_NAME"; \
	cp "$(SOURCE_BINARY)" "dist/$$PACKAGE_NAME/"; \
	cp README.md "dist/$$PACKAGE_NAME/" 2>/dev/null || echo "# $(BINARY_NAME)" > "dist/$$PACKAGE_NAME/README.md"; \
	$(MAKE) create-manpage; \
	cp "$(BINARY_NAME).1" "dist/$$PACKAGE_NAME/"; \
	cp Makefile "dist/$$PACKAGE_NAME/"; \
	cd dist && tar czf "$$PACKAGE_NAME.tar.gz" "$$PACKAGE_NAME"; \
	echo "$(GREEN)✓ Package created: dist/$$PACKAGE_NAME.tar.gz$(NC)"

# Show current status
.PHONY: status
status:
	@echo "$(BLUE)Project Status$(NC)"
	@echo "=============="
	@echo ""
	@echo "$(GREEN)Build Status:$(NC)"
	@if [ -f "$(SOURCE_BINARY)" ]; then \
		echo "  ✓ Release binary exists: $(SOURCE_BINARY)"; \
		ls -la "$(SOURCE_BINARY)"; \
	else \
		echo "  ✗ Release binary not built"; \
	fi
	@echo ""
	@echo "$(GREEN)Installation Status:$(NC)"
	@$(MAKE) check
	@echo ""
	@echo "$(GREEN)Development:$(NC)"
	@echo "  Rust version: $$(rustc --version 2>/dev/null || echo 'Not installed')"
	@echo "  Cargo version: $$(cargo --version 2>/dev/null || echo 'Not installed')"

.DEFAULT_GOAL := help
