#!/usr/bin/env bash
set -euo pipefail

IDENTITY_NAME="KeyPulse Local Code Signing"
KEYCHAIN="${HOME}/Library/Keychains/login.keychain-db"

if security find-identity -v -p codesigning | grep -Fq "\"${IDENTITY_NAME}\""; then
  echo "Using existing code signing identity: ${IDENTITY_NAME}"
  exit 0
fi

WORKDIR="$(mktemp -d)"
trap 'rm -rf "${WORKDIR}"' EXIT

cat > "${WORKDIR}/openssl.cnf" <<'EOF'
[ req ]
default_bits = 2048
prompt = no
default_md = sha256
distinguished_name = dn
x509_extensions = v3_req

[ dn ]
CN = KeyPulse Local Code Signing

[ v3_req ]
basicConstraints = critical,CA:FALSE
keyUsage = critical,digitalSignature
extendedKeyUsage = codeSigning
EOF

openssl req -new -newkey rsa:2048 -nodes -x509 -days 3650 \
  -keyout "${WORKDIR}/key.pem" \
  -out "${WORKDIR}/cert.pem" \
  -config "${WORKDIR}/openssl.cnf"

security add-trusted-cert -r trustRoot -p codeSign -k "${KEYCHAIN}" "${WORKDIR}/cert.pem"

openssl pkcs12 -export \
  -inkey "${WORKDIR}/key.pem" \
  -in "${WORKDIR}/cert.pem" \
  -out "${WORKDIR}/keypulse-codesign.p12" \
  -passout pass:keypulse-local

security import "${WORKDIR}/keypulse-codesign.p12" \
  -k "${KEYCHAIN}" \
  -P keypulse-local \
  -T /usr/bin/codesign \
  -T /usr/bin/security

echo "Created code signing identity: ${IDENTITY_NAME}"
