# Makefile for the aide-rs project.
# Provides common commands for building, testing, and linting.

.PHONY: all build check test fmt clippy clean run

# Set the default goal to 'all' for a comprehensive check.
.DEFAULT_GOAL := all

# Variables
CARGO := cargo

# Targets

# `make all` or `make`: Runs the most common checks in sequence.
all: check fmt clippy test

# `make build`: Compiles the project.
build:
	$(CARGO) build

# `make check`: Checks the project for errors without building.
check:
	$(CARGO) check

# `make test`: Runs all tests.
# The --test-threads=1 flag is used to prevent race conditions in tests
# that modify the environment, ensuring sequential execution.
test:
	$(CARGO) test -- --test-threads=1

# `make fmt`: Checks if the code is formatted according to Rust style guidelines.
fmt:
	$(CARGO) fmt --all -- --check

# `make clippy`: Lints the code for common mistakes and style issues.
# -D warnings promotes warnings to errors, enforcing high code quality.
clippy:
	$(CARGO) clippy -- -D warnings

# `make run`: Runs the application.
# Note: This will require additional arguments for the CLI.
# Example: make run ARGS="run plan --prompt prompts/lancedb_example.yml"
run:
	$(CARGO) run -- $(ARGS)

# `make clean`: Removes the target directory.
clean:
	$(CARGO) clean

# `make install`: Installs the program and its tools
install:
	$(CARGO) build --release && cp ./target/release/aide-rs ~/.local/bin/ && cp ./target/release/doc-retriever ~/.local/bin/
