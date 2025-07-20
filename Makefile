# TFTP Server Makefile
# Simple TFTP server for Linux systems

# Variables
BINARY_NAME = tftpd-linux
PREFIX ?= /usr/local
BINDIR = $(PREFIX)/bin
MANDIR = $(PREFIX)/share/man/man1
TARGET_DIR = target/release
SOURCE_BINARY = $(TARGET_DIR)/$(BINARY_NAME)
MANPAGE_SRC = $(BINARY_NAME).1.in
MANPAGE_DST = $(BINARY_NAME).1

# Colors for pretty output
# Check if stdout is a TTY and enable colors accordingly
ifneq (,$(shell test -t 1 && echo 1))
  GREEN = \033[0;32m
  BLUE = \033[0;34m
  YELLOW = \033[1;33m
  RED = \033[0;31m
  NC = \033[0m # No Color
else
  GREEN =
  BLUE =
  YELLOW =
  RED =
  NC =
endif

# Default target
.PHONY: all
all: build

# Help target
.PHONY: help
help:
	@printf "%b\n" "$(BLUE)TFTP Server Build System$(NC)"
	@printf "%b\n" "========================="
	@printf "\n"
	@printf "%b\n" "$(GREEN)Available targets:$(NC)"
	@printf "  %b\n" "$(YELLOW)build$(NC)     - Build the release binary"
	@printf "  %b\n" "$(YELLOW)install$(NC)   - Install the program (requires 'make build' first)"
	@printf "  %b\n" "$(YELLOW)uninstall$(NC) - Remove the program (requires sudo)"
	@printf "  %b\n" "$(YELLOW)clean$(NC)     - Clean build artifacts"
	@printf "  %b\n" "$(YELLOW)test$(NC)      - Run tests"
	@printf "  %b\n" "$(YELLOW)package$(NC)   - Create a distributable package"
	@printf "  %b\n" "$(YELLOW)help$(NC)      - Show this help"
	@printf "\n"
	@printf "%b\n" "$(GREEN)Correct Usage:$(NC)"
	@printf "  %s\n" "1. make build"
	@printf "  %s\n" "2. sudo make install"

# Check if Rust is installed
.PHONY: check-rust
check-rust:
	@which cargo >/dev/null 2>&1 || { \
		printf "%b\n" "$(RED)Error: Rust/Cargo not found!$(NC)"; \
		printf "Please install Rust from: https://rustup.rs/\n"; \
		exit 1; \
	}

# Build release binary
.PHONY: build
build: check-rust
	@printf "%b\n" "$(BLUE)Building release binary...$(NC)"
	@cargo build --release
	@printf "%b\n" "$(GREEN)✓ Build completed: $(SOURCE_BINARY)$(NC)"

# Generate the man page from the template
$(MANPAGE_DST): $(MANPAGE_SRC)
	@printf "%b\n" "$(BLUE)Creating manual page...$(NC)"
	@sed 's/%%DATE%%/$(shell date '+%B %Y')/' $< > $@

# --- IMPORTANT: 'install' target does not depend on 'build' ---
.PHONY: install
install: $(MANPAGE_DST)
	@printf "%b\n" "$(BLUE)Installing $(BINARY_NAME)...$(NC)"
	@# Fail if the binary hasn't been built yet
	@if [ ! -f "$(SOURCE_BINARY)" ]; then \
		printf "%b\n" "$(RED)Error: Binary not found at '$(SOURCE_BINARY)'!$(NC)"; \
		printf "Please run '%b' as a normal user first.\n" "$(YELLOW)make build$(NC)"; \
		exit 1; \
	fi
	@# Check for root permissions before proceeding
	@if [ "$(shell id -u)" != "0" ]; then \
		printf "%b\n" "$(RED)Error: Install requires root privileges.$(NC)"; \
		printf "Please run this command as '%b'.\n" "$(YELLOW)sudo make install$(NC)"; \
		exit 1; \
	fi
	@# Create directories and install files
	@install -d -m 755 "$(BINDIR)"
	@install -d -m 755 "$(MANDIR)"
	@install -m 755 "$(SOURCE_BINARY)" "$(BINDIR)/$(BINARY_NAME)"
	@install -m 644 "$(MANPAGE_DST)" "$(MANDIR)/$(MANPAGE_DST)"
	@# Update man database if mandb exists
	@if command -v mandb >/dev/null 2>&1; then \
		printf "%b\n" "$(BLUE)Updating manual database...$(NC)"; \
		mandb -q 2>/dev/null || true; \
	fi
	@printf "%b\n" "$(GREEN)✓ Installation completed successfully!$(NC)"
	@printf "You can now run '%b' from anywhere.\n" "$(YELLOW)$(BINARY_NAME)$(NC)"

# Uninstall from system
.PHONY: uninstall
uninstall:
	@printf "%b\n" "$(BLUE)Uninstalling $(BINARY_NAME)...$(NC)"
	@# Check for root permissions before proceeding
	@if [ "$(shell id -u)" != "0" ]; then \
		printf "%b\n" "$(RED)Error: Uninstall requires root privileges.$(NC)"; \
		printf "Please run this command as '%b'.\n" "$(YELLOW)sudo make uninstall$(NC)"; \
		exit 1; \
	fi
	@rm -f "$(BINDIR)/$(BINARY_NAME)"
	@rm -f "$(MANDIR)/$(MANPAGE_DST)"
	@if command -v mandb >/dev/null 2>&1; then \
		printf "%b\n" "$(BLUE)Updating manual database...$(NC)"; \
		mandb -q 2>/dev/null || true; \
	fi
	@printf "%b\n" "$(GREEN)✓ Uninstallation completed!$(NC)"

# Clean build artifacts
.PHONY: clean
clean:
	@printf "%b\n" "$(BLUE)Cleaning build artifacts...$(NC)"
	@cargo clean
	@rm -f $(MANPAGE_DST)
	@printf "%b\n" "$(GREEN)✓ Clean completed$(NC)"

# Run tests
.PHONY: test
test: check-rust
	@printf "%b\n" "$(BLUE)Running tests...$(NC)"
	@cargo test
	@printf "%b\n" "$(GREEN)✓ Tests completed$(NC)"

# Package for distribution
.PHONY: package
package: build $(MANPAGE_DST)
	@printf "%b\n" "$(BLUE)Creating distribution package...$(NC)"
	@VERSION=$$(cargo pkgid | cut -d'#' -f2 | sed 's/.*v//'); \
	PACKAGE_NAME="$(BINARY_NAME)-$$VERSION-x86_64-linux"; \
	PACKAGE_DIR="dist/$$PACKAGE_NAME"; \
	mkdir -p "$$PACKAGE_DIR"; \
	cp "$(SOURCE_BINARY)" "$$PACKAGE_DIR/"; \
	cp README.md "$$PACKAGE_DIR/"; \
	cp "$(MANPAGE_DST)" "$$PACKAGE_DIR/"; \
	cp Makefile "$$PACKAGE_DIR/"; \
	cd dist && tar czf "$$PACKAGE_NAME.tar.gz" "$$PACKAGE_NAME"; \
	printf "%b\n" "$(GREEN)✓ Package created: dist/$$PACKAGE_NAME.tar.gz$(NC)"

.DEFAULT_GOAL := help
