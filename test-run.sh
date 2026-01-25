#!/bin/bash
set -e

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}╔═══════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║           VltView - Test Run Script                          ║${NC}"
echo -e "${BLUE}╚═══════════════════════════════════════════════════════════════╝${NC}"
echo ""

# Function to print colored messages
info() {
    echo -e "${BLUE}ℹ ${NC}$1"
}

success() {
    echo -e "${GREEN}✓${NC} $1"
}

warning() {
    echo -e "${YELLOW}⚠${NC} $1"
}

error() {
    echo -e "${RED}✗${NC} $1"
}

# Check if we're in the right directory
if [ ! -f "Cargo.toml" ]; then
    error "Please run this script from the vltview project root directory"
    exit 1
fi

# Parse command line arguments
MODE=${1:-"fake"}

case $MODE in
    "fake")
        info "Running in FAKE mode (no real Vault connection)"
        echo ""

        # Set fake credentials
        export VAULT_ADDR="http://localhost:8200"
        export VAULT_TOKEN="fake-test-token-12345"

        success "Set fake Vault credentials:"
        echo "  VAULT_ADDR=$VAULT_ADDR"
        echo "  VAULT_TOKEN=$VAULT_TOKEN"
        echo ""

        info "Building VltView..."
        cargo build --quiet 2>&1 | grep -E "Compiling|Finished" || true
        success "Build complete"
        echo ""

        warning "Note: Application will show UI but won't connect to real Vault"
        echo ""
        info "Starting VltView... (Press 'q' or Ctrl+C to quit)"
        echo ""
        sleep 2

        # Run the application
        cargo run --quiet -- --debug
        ;;

    "dev")
        info "Running in DEV mode (with Vault dev server)"
        echo ""

        # Check if vault is installed
        if ! command -v vault &> /dev/null; then
            error "Vault is not installed!"
            echo ""
            echo "Install Vault:"
            echo "  macOS:    brew install vault"
            echo "  Ubuntu:   sudo apt-get install vault"
            echo "  Docker:   docker pull vault:latest"
            exit 1
        fi

        success "Vault is installed: $(vault version | head -1)"
        echo ""

        # Check if Vault dev server is already running
        if curl -s http://127.0.0.1:8200/v1/sys/health &> /dev/null; then
            warning "Vault dev server is already running at http://127.0.0.1:8200"
        else
            info "Starting Vault dev server..."
            echo ""

            # Start Vault in background
            vault server -dev -dev-root-token-id="root" > /tmp/vault-dev.log 2>&1 &
            VAULT_PID=$!

            # Save PID for cleanup
            echo $VAULT_PID > /tmp/vault-dev.pid

            # Wait for Vault to be ready
            info "Waiting for Vault to start..."
            for i in {1..10}; do
                if curl -s http://127.0.0.1:8200/v1/sys/health &> /dev/null; then
                    success "Vault dev server started (PID: $VAULT_PID)"
                    break
                fi
                sleep 1
            done
        fi

        # Set environment
        export VAULT_ADDR='http://127.0.0.1:8200'
        export VAULT_TOKEN='root'

        success "Vault credentials set:"
        echo "  VAULT_ADDR=$VAULT_ADDR"
        echo "  VAULT_TOKEN=$VAULT_TOKEN"
        echo ""

        # Create test secrets
        info "Creating test secrets..."

        vault kv put secret/myapp/database \
            password=secret123 \
            username=admin \
            host=localhost \
            port=5432 > /dev/null
        success "Created secret/myapp/database"

        vault kv put secret/myapp/api \
            key=abc123xyz789 \
            endpoint=https://api.example.com \
            version=v2 > /dev/null
        success "Created secret/myapp/api"

        vault kv put secret/prod/database \
            password=prod-secret \
            host=db.prod.com \
            port=5432 > /dev/null
        success "Created secret/prod/database"

        vault kv put secret/test/credentials \
            username=testuser \
            password=testpass > /dev/null
        success "Created secret/test/credentials"

        echo ""
        info "Test secrets structure:"
        echo "  /secret/myapp/database"
        echo "  /secret/myapp/api"
        echo "  /secret/prod/database"
        echo "  /secret/test/credentials"
        echo ""

        info "Building VltView..."
        cargo build --quiet 2>&1 | grep -E "Compiling|Finished" || true
        success "Build complete"
        echo ""

        info "Starting VltView... (Press 'q' or Ctrl+C to quit)"
        echo ""
        sleep 2

        # Run the application
        cargo run --quiet -- /secret/myapp

        # Cleanup on exit
        echo ""
        info "Cleaning up..."
        if [ -f /tmp/vault-dev.pid ]; then
            VAULT_PID=$(cat /tmp/vault-dev.pid)
            if ps -p $VAULT_PID > /dev/null 2>&1; then
                kill $VAULT_PID
                success "Stopped Vault dev server"
            fi
            rm /tmp/vault-dev.pid
        fi
        ;;

    "docker")
        info "Running in DOCKER mode (Vault in Docker container)"
        echo ""

        # Check if docker is installed
        if ! command -v docker &> /dev/null; then
            error "Docker is not installed!"
            echo ""
            echo "Install Docker: https://docs.docker.com/get-docker/"
            exit 1
        fi

        success "Docker is installed"
        echo ""

        # Check if container exists
        if docker ps -a --format '{{.Names}}' | grep -q '^vault-dev$'; then
            warning "Removing existing vault-dev container..."
            docker rm -f vault-dev > /dev/null 2>&1
        fi

        info "Starting Vault in Docker container..."
        docker run -d \
            --name vault-dev \
            --cap-add=IPC_LOCK \
            -e 'VAULT_DEV_ROOT_TOKEN_ID=myroot' \
            -p 8200:8200 \
            vault:latest > /dev/null

        success "Vault container started"
        echo ""

        # Wait for Vault to be ready
        info "Waiting for Vault to be ready..."
        for i in {1..15}; do
            if curl -s http://127.0.0.1:8200/v1/sys/health &> /dev/null; then
                success "Vault is ready"
                break
            fi
            sleep 1
        done
        echo ""

        # Set environment
        export VAULT_ADDR='http://127.0.0.1:8200'
        export VAULT_TOKEN='myroot'

        success "Vault credentials set:"
        echo "  VAULT_ADDR=$VAULT_ADDR"
        echo "  VAULT_TOKEN=$VAULT_TOKEN"
        echo ""

        # Install vault CLI in container or use docker exec
        info "Creating test secrets..."

        docker exec vault-dev vault kv put secret/myapp/database \
            password=secret123 \
            username=admin \
            host=localhost \
            port=5432 > /dev/null
        success "Created secret/myapp/database"

        docker exec vault-dev vault kv put secret/myapp/api \
            key=abc123xyz789 \
            endpoint=https://api.example.com > /dev/null
        success "Created secret/myapp/api"

        echo ""

        info "Building VltView..."
        cargo build --quiet 2>&1 | grep -E "Compiling|Finished" || true
        success "Build complete"
        echo ""

        info "Starting VltView... (Press 'q' or Ctrl+C to quit)"
        echo ""
        sleep 2

        # Run the application
        cargo run --quiet -- /secret/myapp

        # Cleanup
        echo ""
        info "Cleaning up..."
        docker stop vault-dev > /dev/null 2>&1
        docker rm vault-dev > /dev/null 2>&1
        success "Stopped and removed Vault container"
        ;;

    "test")
        info "Running TESTS only"
        echo ""

        cargo test 2>&1 | grep -E "test result:|running|passed"
        ;;

    *)
        error "Unknown mode: $MODE"
        echo ""
        echo "Usage: $0 [MODE]"
        echo ""
        echo "Available modes:"
        echo "  fake     - Run with fake credentials (no real Vault)"
        echo "  dev      - Run with local Vault dev server"
        echo "  docker   - Run with Vault in Docker"
        echo "  test     - Run unit tests only"
        echo ""
        echo "Examples:"
        echo "  $0 fake"
        echo "  $0 dev"
        echo "  $0 docker"
        echo "  $0 test"
        exit 1
        ;;
esac

echo ""
success "Done!"
