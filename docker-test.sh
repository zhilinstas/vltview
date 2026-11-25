#!/bin/bash
set -e

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

echo -e "${BLUE}╔═══════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║           VltView + Docker Vault - Test Script               ║${NC}"
echo -e "${BLUE}╚═══════════════════════════════════════════════════════════════╝${NC}"
echo ""

# Detect docker compose command
if command -v docker-compose &> /dev/null; then
    DOCKER_COMPOSE="docker-compose"
    echo -e "${GREEN}✓${NC} Found: docker-compose (v1)"
elif docker compose version &> /dev/null; then
    DOCKER_COMPOSE="docker compose"
    echo -e "${GREEN}✓${NC} Found: docker compose (v2)"
else
    echo -e "${RED}✗ Docker Compose not found!${NC}"
    echo "Please install Docker Desktop or Docker Compose"
    exit 1
fi

MODE=${1:-"start"}

case $MODE in
    "start"|"up")
        echo ""
        echo -e "${BLUE}Starting Vault...${NC}"
        $DOCKER_COMPOSE up -d

        echo ""
        echo -e "${YELLOW}Waiting for Vault to initialize (10 seconds)...${NC}"
        sleep 10

        echo ""
        echo -e "${BLUE}Checking status...${NC}"
        $DOCKER_COMPOSE ps

        echo ""
        echo -e "${BLUE}Viewing initialization logs...${NC}"
        $DOCKER_COMPOSE logs vault-init | tail -20

        echo ""
        echo -e "${GREEN}╔═══════════════════════════════════════════════════════════════╗${NC}"
        echo -e "${GREEN}║                    ✓ Vault is Ready!                         ║${NC}"
        echo -e "${GREEN}╚═══════════════════════════════════════════════════════════════╝${NC}"
        echo ""
        echo -e "${YELLOW}To run VltView:${NC}"
        echo ""
        echo "  export VAULT_ADDR='http://localhost:8200'"
        echo "  export VAULT_TOKEN='root'"
        echo "  cargo run -- /myapp"
        echo ""
        echo -e "${YELLOW}Or run this script again:${NC}"
        echo "  ./docker-test.sh run"
        echo ""
        ;;

    "run")
        echo ""
        echo -e "${BLUE}Checking if Vault is running...${NC}"
        if ! $DOCKER_COMPOSE ps | grep -q "vltview-vault.*Up"; then
            echo -e "${YELLOW}Vault is not running. Starting it...${NC}"
            $0 start
            echo ""
        fi

        echo -e "${BLUE}Running VltView...${NC}"
        echo ""
        export VAULT_ADDR='http://localhost:8200'
        export VAULT_TOKEN='root'
        cargo run --quiet
        ;;

    "stop"|"down")
        echo ""
        echo -e "${BLUE}Stopping Vault...${NC}"
        $DOCKER_COMPOSE down
        echo ""
        echo -e "${GREEN}✓ Vault stopped${NC}"
        ;;

    "logs")
        echo ""
        echo -e "${BLUE}Showing logs (Ctrl+C to exit)...${NC}"
        echo ""
        $DOCKER_COMPOSE logs -f
        ;;

    "status")
        echo ""
        $DOCKER_COMPOSE ps
        echo ""
        ;;

    "clean")
        echo ""
        echo -e "${YELLOW}Removing all containers and volumes...${NC}"
        $DOCKER_COMPOSE down -v
        echo ""
        echo -e "${GREEN}✓ Cleaned up${NC}"
        ;;

    "restart")
        echo ""
        echo -e "${BLUE}Restarting Vault...${NC}"
        $DOCKER_COMPOSE restart
        echo ""
        echo -e "${GREEN}✓ Restarted${NC}"
        ;;

    "shell")
        echo ""
        echo -e "${BLUE}Opening shell in Vault container...${NC}"
        echo ""
        $DOCKER_COMPOSE exec vault /bin/sh
        ;;

    *)
        echo ""
        echo "Usage: $0 [COMMAND]"
        echo ""
        echo "Commands:"
        echo "  start, up     - Start Vault (default)"
        echo "  run           - Start Vault and run VltView"
        echo "  stop, down    - Stop Vault"
        echo "  logs          - Show logs"
        echo "  status        - Show status"
        echo "  restart       - Restart Vault"
        echo "  clean         - Remove everything (including volumes)"
        echo "  shell         - Open shell in Vault container"
        echo ""
        echo "Examples:"
        echo "  $0 start      # Start Vault"
        echo "  $0 run        # Start Vault and run VltView"
        echo "  $0 stop       # Stop Vault"
        echo ""
        ;;
esac
