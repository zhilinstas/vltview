#!/bin/bash
set -e

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo -e "${BLUE}╔═══════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║           HashiCorp Vault - Initialization Script            ║${NC}"
echo -e "${BLUE}╚═══════════════════════════════════════════════════════════════╝${NC}"
echo ""

# Check if vault CLI is available
if ! command -v vault &> /dev/null; then
    echo -e "${RED}✗ Vault CLI not found!${NC}"
    echo "Please install vault or run this script inside the vault container"
    exit 1
fi

# Default values
VAULT_ADDR=${VAULT_ADDR:-"http://127.0.0.1:8200"}
VAULT_TOKEN=${VAULT_TOKEN:-"root"}

echo -e "${BLUE}ℹ${NC} Vault Address: $VAULT_ADDR"
echo -e "${BLUE}ℹ${NC} Vault Token: $VAULT_TOKEN"
echo ""

# Wait for Vault to be ready
echo -e "${YELLOW}⏳${NC} Waiting for Vault to be ready..."
for i in {1..30}; do
    if vault status &> /dev/null; then
        echo -e "${GREEN}✓${NC} Vault is ready!"
        break
    fi
    if [ $i -eq 30 ]; then
        echo -e "${RED}✗ Timeout waiting for Vault${NC}"
        exit 1
    fi
    sleep 1
done
echo ""

echo -e "${BLUE}📝 Creating test secrets...${NC}"
echo ""

# ============================================
# Application Secrets
# ============================================
echo -e "${YELLOW}Creating application secrets...${NC}"

vault kv put secret/myapp/database \
    password=secret123 \
    username=admin \
    host=localhost \
    port=5432 \
    database=myapp_db \
    connection_string="postgresql://admin:secret123@localhost:5432/myapp_db" \
    max_connections=100 \
    ssl_mode=disable

vault kv put secret/myapp/api \
    key=abc123xyz789 \
    endpoint=https://api.example.com \
    version=v2 \
    timeout=30 \
    retries=3 \
    rate_limit=100

vault kv put secret/myapp/redis \
    host=redis.example.com \
    port=6379 \
    password=redis-secret \
    database=0 \
    max_retries=3

vault kv put secret/myapp/smtp \
    host=smtp.example.com \
    port=587 \
    username=noreply@example.com \
    password=smtp-password \
    from=noreply@example.com

vault kv put secret/myapp/oauth \
    client_id=myapp-client-id \
    client_secret=myapp-client-secret \
    redirect_uri=https://myapp.example.com/callback \
    scope="read write"

echo -e "${GREEN}✓${NC} Created secret/myapp/*"

# ============================================
# Production Secrets
# ============================================
echo -e "${YELLOW}Creating production secrets...${NC}"

vault kv put secret/prod/database \
    password=prod-secret-pass-super-secure \
    username=prod-admin \
    host=db.prod.example.com \
    port=5432 \
    database=production_db \
    ssl_mode=require \
    connection_timeout=10

vault kv put secret/prod/api \
    key=prod-api-key-xyz-very-secure \
    endpoint=https://api.prod.example.com \
    rate_limit=1000 \
    version=v2

vault kv put secret/prod/aws \
    access_key_id=AKIAPRODEXAMPLE123 \
    secret_access_key=prod/secret/key/example/very/long \
    region=us-east-1 \
    bucket=prod-bucket

vault kv put secret/prod/stripe \
    api_key=sk_live_prod_key_example \
    webhook_secret=whsec_prod_webhook \
    publishable_key=pk_live_prod_key

echo -e "${GREEN}✓${NC} Created secret/prod/*"

# ============================================
# Staging Secrets
# ============================================
echo -e "${YELLOW}Creating staging secrets...${NC}"

vault kv put secret/staging/database \
    password=staging-pass \
    username=staging-user \
    host=db.staging.example.com \
    port=5432 \
    database=staging_db \
    ssl_mode=prefer

vault kv put secret/staging/api \
    key=staging-api-key-123 \
    endpoint=https://api.staging.example.com \
    version=v2

echo -e "${GREEN}✓${NC} Created secret/staging/*"

# ============================================
# Test Secrets
# ============================================
echo -e "${YELLOW}Creating test secrets...${NC}"

vault kv put secret/test/credentials \
    username=testuser \
    password=testpass \
    email=test@example.com \
    role=tester

vault kv put secret/test/tokens \
    access_token=test-access-token-123 \
    refresh_token=test-refresh-token-456 \
    expires_in=3600

vault kv put secret/test/database \
    host=localhost \
    port=5432 \
    username=testuser \
    password=testpass \
    database=test_db

# ============================================
# Multiline Test Secrets (for textarea testing)
# ============================================
echo -e "${YELLOW}Creating multiline test secrets...${NC}"

# SSH Private Key
vault kv put secret/test/ssh-key \
    key_name="id_rsa" \
    public_key="ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQC8..." \
    private_key="-----BEGIN RSA PRIVATE KEY-----
MIIEpAIBAAKCAQEAyKmVHwzPc1234567890abcdefghijklm
nopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ1234567
890abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQR
STUVWXYZ1234567890abcdefghijklmnopqrstuvwxyzAB
CDEFGHIJKLMNOPQRSTUVWXYZ1234567890abcdefghijkl
mnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ123456
-----END RSA PRIVATE KEY-----"

# JSON Configuration
vault kv put secret/test/json-config \
    app_name="myapp" \
    config='{
  "database": {
    "host": "localhost",
    "port": 5432,
    "credentials": {
      "username": "admin",
      "password": "secret"
    }
  },
  "api": {
    "endpoints": [
      "https://api1.example.com",
      "https://api2.example.com"
    ],
    "timeout": 30
  },
  "features": {
    "cache": true,
    "logging": true
  }
}'

# YAML Configuration
vault kv put secret/test/yaml-config \
    environment="production" \
    config='apiVersion: v1
kind: ConfigMap
metadata:
  name: app-config
  namespace: production
data:
  database.host: db.prod.com
  database.port: "5432"
  redis.enabled: "true"
  features: |
    - authentication
    - caching
    - monitoring'

# Shell Script
vault kv put secret/test/deploy-script \
    name="deploy.sh" \
    script='#!/bin/bash
set -e

echo "Starting deployment..."

# Check prerequisites
if [ ! -f "config.yml" ]; then
  echo "Error: config.yml not found"
  exit 1
fi

# Deploy application
kubectl apply -f manifests/
kubectl rollout status deployment/myapp

echo "Deployment complete!"'

# Docker Compose
vault kv put secret/test/docker-compose \
    project="myapp" \
    compose='version: "3.8"

services:
  web:
    image: nginx:latest
    ports:
      - "80:80"
    environment:
      - NGINX_HOST=example.com
    volumes:
      - ./html:/usr/share/nginx/html

  db:
    image: postgres:14
    environment:
      POSTGRES_PASSWORD: secret
      POSTGRES_DB: myapp
    volumes:
      - db-data:/var/lib/postgresql/data

volumes:
  db-data:'

# Python Code
vault kv put secret/test/python-script \
    filename="process.py" \
    code='import json
import sys
from datetime import datetime

def process_data(input_file):
    """Process input data and return results."""
    try:
        with open(input_file, "r") as f:
            data = json.load(f)

        results = {
            "processed_at": datetime.now().isoformat(),
            "records": len(data),
            "status": "success"
        }

        return results
    except Exception as e:
        return {"status": "error", "message": str(e)}

if __name__ == "__main__":
    result = process_data(sys.argv[1])
    print(json.dumps(result, indent=2))'

# SQL Migration
vault kv put secret/test/sql-migration \
    version="001" \
    up_sql='-- Migration: Add user authentication
-- Version: 001
-- Date: 2024-01-01

CREATE TABLE IF NOT EXISTS users (
    id SERIAL PRIMARY KEY,
    username VARCHAR(255) UNIQUE NOT NULL,
    email VARCHAR(255) UNIQUE NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_users_username ON users(username);
CREATE INDEX idx_users_email ON users(email);

-- Insert admin user
INSERT INTO users (username, email, password_hash)
VALUES ('"'"'admin'"'"', '"'"'admin@example.com'"'"', '"'"'$2a$10$example'"'"');' \
    down_sql='-- Rollback: Remove user authentication
-- Version: 001

DROP INDEX IF EXISTS idx_users_email;
DROP INDEX IF EXISTS idx_users_username;
DROP TABLE IF EXISTS users;'

echo -e "${GREEN}✓${NC} Created secret/test/* (including multiline examples)"

# ============================================
# Development Secrets
# ============================================
echo -e "${YELLOW}Creating development secrets...${NC}"

vault kv put secret/dev/database \
    password=devpass \
    username=devuser \
    host=localhost \
    port=5432 \
    database=dev_db

vault kv put secret/dev/aws \
    access_key_id=AKIAIOSFODNN7EXAMPLE \
    secret_access_key=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY \
    region=us-east-1 \
    bucket=dev-bucket

vault kv put secret/dev/github \
    token=ghp_devtoken123example \
    org=mycompany \
    repo=myapp

vault kv put secret/dev/slack \
    webhook=https://hooks.slack.com/services/DEV/WEBHOOK/URL \
    channel=#dev-notifications \
    username=devbot

echo -e "${GREEN}✓${NC} Created secret/dev/*"

# ============================================
# Nested Company Structure
# ============================================
echo -e "${YELLOW}Creating company secrets...${NC}"

vault kv put secret/company/hr/payroll \
    system=workday \
    api_key=hr-api-key-123 \
    endpoint=https://api.workday.com \
    username=hr-admin

vault kv put secret/company/hr/benefits \
    provider=benefits-co \
    api_key=benefits-key-456 \
    contact=benefits@company.com

vault kv put secret/company/finance/accounting \
    system=quickbooks \
    api_key=finance-api-key-456 \
    company_id=12345

vault kv put secret/company/finance/banking \
    account_number=****1234 \
    routing_number=987654321 \
    bank_name="Example Bank"

vault kv put secret/company/engineering/github \
    token=ghp_companytoken123 \
    org=company-org \
    teams=engineering,devops

vault kv put secret/company/engineering/jenkins \
    url=https://jenkins.company.com \
    username=jenkins-admin \
    api_token=jenkins-api-token-123

vault kv put secret/company/marketing/mailchimp \
    api_key=mailchimp-key-123 \
    list_id=abc123 \
    from_email=marketing@company.com

vault kv put secret/company/sales/salesforce \
    instance_url=https://company.salesforce.com \
    username=sales-admin@company.com \
    password=salesforce-pass \
    security_token=salesforce-token

echo -e "${GREEN}✓${NC} Created secret/company/*"

# ============================================
# Multi-level Nested Secrets
# ============================================
echo -e "${YELLOW}Creating deeply nested secrets...${NC}"

vault kv put secret/services/backend/auth-service \
    database_url=postgresql://auth:pass@db:5432/auth \
    jwt_secret=super-secret-jwt-key \
    port=8080

vault kv put secret/services/backend/payment-service \
    database_url=postgresql://payment:pass@db:5432/payment \
    stripe_key=sk_test_payment_key \
    port=8081

vault kv put secret/services/frontend/web-app \
    api_endpoint=https://api.example.com \
    analytics_key=UA-12345678-1 \
    sentry_dsn=https://sentry.example.com/project

vault kv put secret/services/frontend/mobile-app \
    api_endpoint=https://api.example.com \
    app_id=com.example.mobile \
    push_key=mobile-push-key

echo -e "${GREEN}✓${NC} Created secret/services/*"

echo ""
echo -e "${BLUE}🔐 Creating test policies...${NC}"
echo ""

# ============================================
# Readonly Policy
# ============================================
vault policy write readonly-policy - <<EOF
# Read-only access to all secrets
path "secret/data/*" {
  capabilities = ["read", "list"]
}

path "secret/metadata/*" {
  capabilities = ["read", "list"]
}

# List policies
path "sys/policies/acl" {
  capabilities = ["list"]
}
EOF

echo -e "${GREEN}✓${NC} Created readonly-policy"

# ============================================
# Admin Policy
# ============================================
vault policy write admin-policy - <<EOF
# Full access to secrets
path "secret/*" {
  capabilities = ["create", "read", "update", "delete", "list"]
}

# Full access to policies
path "sys/policies/*" {
  capabilities = ["create", "read", "update", "delete", "list"]
}

# System health and capabilities
path "sys/health" {
  capabilities = ["read"]
}

path "sys/capabilities-self" {
  capabilities = ["read"]
}
EOF

echo -e "${GREEN}✓${NC} Created admin-policy"

# ============================================
# Developer Policy
# ============================================
vault policy write dev-policy - <<EOF
# Full access to dev secrets
path "secret/data/dev/*" {
  capabilities = ["create", "read", "update", "delete", "list"]
}

path "secret/metadata/dev/*" {
  capabilities = ["read", "list"]
}

# Full access to test secrets
path "secret/data/test/*" {
  capabilities = ["create", "read", "update", "delete", "list"]
}

path "secret/metadata/test/*" {
  capabilities = ["read", "list"]
}

# Read-only access to staging
path "secret/data/staging/*" {
  capabilities = ["read", "list"]
}
EOF

echo -e "${GREEN}✓${NC} Created dev-policy"

# ============================================
# Production Read-Only Policy
# ============================================
vault policy write prod-readonly-policy - <<EOF
# Read-only access to production secrets
path "secret/data/prod/*" {
  capabilities = ["read", "list"]
}

path "secret/metadata/prod/*" {
  capabilities = ["read", "list"]
}
EOF

echo -e "${GREEN}✓${NC} Created prod-readonly-policy"

# ============================================
# App-Specific Policy
# ============================================
vault policy write myapp-policy - <<EOF
# Application-specific access
path "secret/data/myapp/*" {
  capabilities = ["read", "list"]
}

path "secret/metadata/myapp/*" {
  capabilities = ["read", "list"]
}

# Read services config
path "secret/data/services/*" {
  capabilities = ["read", "list"]
}
EOF

echo -e "${GREEN}✓${NC} Created myapp-policy"

echo ""
echo -e "${BLUE}╔═══════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║                  ✅ Initialization Complete!                  ║${NC}"
echo -e "${BLUE}╚═══════════════════════════════════════════════════════════════╝${NC}"
echo ""

echo -e "${GREEN}📊 Created Secrets:${NC}"
echo "  ├── /secret/myapp/*         (5 secrets)"
echo "  ├── /secret/prod/*          (4 secrets)"
echo "  ├── /secret/staging/*       (2 secrets)"
echo "  ├── /secret/test/*          (10 secrets - includes 7 multiline examples)"
echo "  ├── /secret/dev/*           (4 secrets)"
echo "  ├── /secret/company/*       (8 secrets in nested structure)"
echo "  └── /secret/services/*      (4 secrets in nested structure)"
echo ""
echo -e "${YELLOW}📝 Multiline Examples in /secret/test/*:${NC}"
echo "  ├── ssh-key          (SSH private key)"
echo "  ├── json-config      (JSON configuration)"
echo "  ├── yaml-config      (YAML configuration)"
echo "  ├── deploy-script    (Bash deployment script)"
echo "  ├── docker-compose   (Docker compose file)"
echo "  ├── python-script    (Python code)"
echo "  └── sql-migration    (SQL up/down migration)"
echo ""

echo -e "${GREEN}🔐 Created Policies:${NC}"
echo "  ├── readonly-policy"
echo "  ├── admin-policy"
echo "  ├── dev-policy"
echo "  ├── prod-readonly-policy"
echo "  └── myapp-policy"
echo ""

echo -e "${GREEN}📝 Quick Commands:${NC}"
echo "  # List all secrets"
echo "  vault kv list secret/"
echo ""
echo "  # List policies"
echo "  vault policy list"
echo ""
echo "  # Get a secret"
echo "  vault kv get secret/myapp/database"
echo ""
echo "  # List nested path"
echo "  vault kv list secret/company/"
echo ""

echo -e "${BLUE}🚀 Test VltView:${NC}"
echo "  export VAULT_ADDR='$VAULT_ADDR'"
echo "  export VAULT_TOKEN='$VAULT_TOKEN'"
echo "  cargo run -- /secret/myapp"
echo ""

echo -e "${GREEN}✨ Happy testing!${NC}"
