.PHONY: help build run test clean docker-up docker-down docker-logs docker-clean fmt clippy check install dev docs

# Default target
.DEFAULT_GOAL := help

# Colors for output
BLUE := \033[0;34m
GREEN := \033[0;32m
YELLOW := \033[1;33m
RED := \033[0;31m
NC := \033[0m # No Color

# Project variables
PROJECT_NAME := vltview
BINARY_NAME := vltview
CARGO := cargo
# Detect docker compose command (new: docker compose, old: docker-compose)
DOCKER_COMPOSE := $(shell which docker-compose 2>/dev/null || echo "docker compose")

help: ## Show this help message
	@echo "$(BLUE)╔═══════════════════════════════════════════════════════════════╗$(NC)"
	@echo "$(BLUE)║              VltView - Makefile Commands                     ║$(NC)"
	@echo "$(BLUE)╚═══════════════════════════════════════════════════════════════╝$(NC)"
	@echo ""
	@echo "$(GREEN)Available commands:$(NC)"
	@echo ""
	@awk 'BEGIN {FS = ":.*##"; printf ""} /^[a-zA-Z_-]+:.*?##/ { printf "  $(YELLOW)%-20s$(NC) %s\n", $$1, $$2 }' $(MAKEFILE_LIST)
	@echo ""

##@ Building

build: ## Build the project in debug mode
	@echo "$(BLUE)Building $(PROJECT_NAME)...$(NC)"
	$(CARGO) build
	@echo "$(GREEN)✓ Build complete$(NC)"

build-release: ## Build the project in release mode (optimized)
	@echo "$(BLUE)Building $(PROJECT_NAME) in release mode...$(NC)"
	$(CARGO) build --release
	@echo "$(GREEN)✓ Release build complete$(NC)"

check: ## Check compilation without building
	@echo "$(BLUE)Checking compilation...$(NC)"
	$(CARGO) check
	@echo "$(GREEN)✓ Check complete$(NC)"

##@ Running

run: ## Run the application with fake credentials
	@echo "$(BLUE)Running $(PROJECT_NAME)...$(NC)"
	@export VAULT_ADDR='http://localhost:8200' && \
	 export VAULT_TOKEN='test-token' && \
	 $(CARGO) run

run-debug: ## Run with debug logging
	@echo "$(BLUE)Running $(PROJECT_NAME) with debug logging...$(NC)"
	@export VAULT_ADDR='http://localhost:8200' && \
	 export VAULT_TOKEN='test-token' && \
	 RUST_LOG=debug $(CARGO) run -- --debug

run-with-docker: ## Run with Docker Vault (starts docker-compose first)
	@echo "$(BLUE)Starting Vault with Docker Compose...$(NC)"
	@$(MAKE) docker-up
	@sleep 5
	@echo "$(BLUE)Running $(PROJECT_NAME)...$(NC)"
	@export VAULT_ADDR='http://localhost:8200' && \
	 export VAULT_TOKEN='root' && \
	 $(CARGO) run -- /secret/myapp

dev: ## Run in development mode with cargo watch
	@echo "$(BLUE)Starting development mode...$(NC)"
	@export VAULT_ADDR='http://localhost:8200' && \
	 export VAULT_TOKEN='test-token' && \
	 cargo watch -x 'run'

##@ Testing

test: ## Run all tests
	@echo "$(BLUE)Running tests...$(NC)"
	$(CARGO) test
	@echo "$(GREEN)✓ All tests passed$(NC)"

test-verbose: ## Run tests with output
	@echo "$(BLUE)Running tests with output...$(NC)"
	$(CARGO) test -- --nocapture

test-specific: ## Run specific test (usage: make test-specific TEST=test_name)
	@echo "$(BLUE)Running test: $(TEST)...$(NC)"
	$(CARGO) test $(TEST) -- --nocapture --exact

test-integration: ## Run integration tests (when available)
	@echo "$(BLUE)Running integration tests...$(NC)"
	$(CARGO) test --test '*'

test-coverage: ## Generate test coverage report (requires cargo-tarpaulin)
	@echo "$(BLUE)Generating coverage report...$(NC)"
	$(CARGO) tarpaulin --out Html --output-dir coverage
	@echo "$(GREEN)✓ Coverage report generated in coverage/$(NC)"

##@ Docker

docker-up: ## Start Vault with Docker Compose
	@echo "$(BLUE)Starting Vault with Docker Compose...$(NC)"
	$(DOCKER_COMPOSE) up -d
	@echo "$(YELLOW)Waiting for Vault to initialize...$(NC)"
	@sleep 3
	@echo "$(GREEN)✓ Vault is ready at http://localhost:8200$(NC)"
	@echo "$(GREEN)  Token: root$(NC)"

docker-down: ## Stop Docker Compose services
	@echo "$(BLUE)Stopping Docker Compose services...$(NC)"
	$(DOCKER_COMPOSE) down
	@echo "$(GREEN)✓ Services stopped$(NC)"

docker-restart: ## Restart Docker Compose services
	@echo "$(BLUE)Restarting Docker Compose services...$(NC)"
	$(DOCKER_COMPOSE) restart
	@echo "$(GREEN)✓ Services restarted$(NC)"

docker-logs: ## Show Docker Compose logs
	$(DOCKER_COMPOSE) logs -f

docker-logs-init: ## Show initialization logs
	$(DOCKER_COMPOSE) logs vault-init

docker-status: ## Show status of Docker services
	$(DOCKER_COMPOSE) ps

docker-clean: ## Stop and remove all containers, networks, and volumes
	@echo "$(BLUE)Cleaning Docker resources...$(NC)"
	$(DOCKER_COMPOSE) down -v
	@echo "$(GREEN)✓ Docker resources cleaned$(NC)"

docker-shell: ## Open shell in Vault container
	@echo "$(BLUE)Opening shell in Vault container...$(NC)"
	$(DOCKER_COMPOSE) exec vault /bin/sh

##@ Code Quality

fmt: ## Format code with rustfmt
	@echo "$(BLUE)Formatting code...$(NC)"
	$(CARGO) fmt
	@echo "$(GREEN)✓ Code formatted$(NC)"

fmt-check: ## Check code formatting
	@echo "$(BLUE)Checking code formatting...$(NC)"
	$(CARGO) fmt -- --check

clippy: ## Run clippy linter
	@echo "$(BLUE)Running clippy...$(NC)"
	$(CARGO) clippy --all-targets --all-features
	@echo "$(GREEN)✓ Clippy check complete$(NC)"

clippy-fix: ## Run clippy and apply fixes
	@echo "$(BLUE)Running clippy with fixes...$(NC)"
	$(CARGO) clippy --fix --allow-dirty --allow-staged

lint: fmt-check clippy ## Run all linters

##@ Cleaning

clean: ## Remove build artifacts
	@echo "$(BLUE)Cleaning build artifacts...$(NC)"
	$(CARGO) clean
	@echo "$(GREEN)✓ Build artifacts removed$(NC)"

clean-all: clean docker-clean ## Clean everything including Docker resources
	@echo "$(GREEN)✓ All cleaned$(NC)"

##@ Installation

install: ## Install the binary to ~/.cargo/bin
	@echo "$(BLUE)Installing $(BINARY_NAME)...$(NC)"
	$(CARGO) install --path .
	@echo "$(GREEN)✓ Installed to ~/.cargo/bin/$(BINARY_NAME)$(NC)"

uninstall: ## Uninstall the binary
	@echo "$(BLUE)Uninstalling $(BINARY_NAME)...$(NC)"
	$(CARGO) uninstall $(BINARY_NAME)
	@echo "$(GREEN)✓ Uninstalled$(NC)"

##@ Documentation

docs: ## Generate and open documentation
	@echo "$(BLUE)Generating documentation...$(NC)"
	$(CARGO) doc --open

docs-build: ## Generate documentation without opening
	@echo "$(BLUE)Generating documentation...$(NC)"
	$(CARGO) doc

##@ Quick Commands

quick-test: ## Quick test: build, test, and run with Docker
	@echo "$(BLUE)╔═══════════════════════════════════════════════════════════════╗$(NC)"
	@echo "$(BLUE)║                    Quick Test Workflow                       ║$(NC)"
	@echo "$(BLUE)╚═══════════════════════════════════════════════════════════════╝$(NC)"
	@echo ""
	@$(MAKE) build
	@echo ""
	@$(MAKE) test
	@echo ""
	@$(MAKE) docker-up
	@echo ""
	@echo "$(GREEN)✓ Ready to test! Run: make run-with-docker$(NC)"

full-check: fmt clippy test build ## Run all checks (format, lint, test, build)
	@echo ""
	@echo "$(GREEN)╔═══════════════════════════════════════════════════════════════╗$(NC)"
	@echo "$(GREEN)║                    ✓ All Checks Passed!                      ║$(NC)"
	@echo "$(GREEN)╚═══════════════════════════════════════════════════════════════╝$(NC)"

pre-commit: full-check ## Run all pre-commit checks
	@echo "$(GREEN)✓ Ready to commit$(NC)"

##@ Information

info: ## Show project information
	@echo "$(BLUE)╔═══════════════════════════════════════════════════════════════╗$(NC)"
	@echo "$(BLUE)║                    Project Information                       ║$(NC)"
	@echo "$(BLUE)╚═══════════════════════════════════════════════════════════════╝$(NC)"
	@echo ""
	@echo "$(YELLOW)Project:$(NC)        $(PROJECT_NAME)"
	@echo "$(YELLOW)Binary:$(NC)         $(BINARY_NAME)"
	@echo "$(YELLOW)Rust Version:$(NC)   $$(rustc --version)"
	@echo "$(YELLOW)Cargo Version:$(NC)  $$(cargo --version)"
	@echo ""
	@echo "$(YELLOW)Files:$(NC)"
	@find src -name '*.rs' | wc -l | xargs echo "  Rust files:   "
	@find src -name '*.rs' -exec cat {} \; | wc -l | xargs echo "  Lines of code:"
	@echo ""
	@echo "$(YELLOW)Tests:$(NC)"
	@$(CARGO) test --quiet 2>&1 | grep "test result" || echo "  Run 'make test' to see results"

version: ## Show version information
	@echo "$(BLUE)Version Information:$(NC)"
	@grep '^version' Cargo.toml | head -1

update: ## Update dependencies
	@echo "$(BLUE)Updating dependencies...$(NC)"
	$(CARGO) update
	@echo "$(GREEN)✓ Dependencies updated$(NC)"

##@ Scripts

script-fake: ## Run test script in fake mode
	@./test-run.sh fake

script-dev: ## Run test script in dev mode
	@./test-run.sh dev

script-docker: ## Run test script in docker mode
	@./test-run.sh docker

script-test: ## Run test script (tests only)
	@./test-run.sh test

##@ Examples

example-commands: ## Show example commands
	@echo "$(BLUE)╔═══════════════════════════════════════════════════════════════╗$(NC)"
	@echo "$(BLUE)║                     Example Commands                         ║$(NC)"
	@echo "$(BLUE)╚═══════════════════════════════════════════════════════════════╝$(NC)"
	@echo ""
	@echo "$(GREEN)Start development:$(NC)"
	@echo "  make docker-up && make run-with-docker"
	@echo ""
	@echo "$(GREEN)Run tests:$(NC)"
	@echo "  make test"
	@echo ""
	@echo "$(GREEN)Full check before commit:$(NC)"
	@echo "  make pre-commit"
	@echo ""
	@echo "$(GREEN)Quick test everything:$(NC)"
	@echo "  make quick-test"
	@echo ""
	@echo "$(GREEN)Clean and rebuild:$(NC)"
	@echo "  make clean && make build"
	@echo ""
