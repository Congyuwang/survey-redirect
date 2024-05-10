#!/bin/bash

key_path="./dev_certs/localhost.key"
cert_path="./dev_certs/localhost.crt"
config_file="./dev_certs/openssl.cnf"

# Create a config file for the certificate generation
cat > "${config_file}" <<EOL
[req]
distinguished_name = req_distinguished_name
req_extensions = req_ext
x509_extensions = v3_req
prompt = no

[req_distinguished_name]
CN = localhost

[req_ext]
subjectAltName = @alt_names

[v3_req]
subjectAltName = @alt_names

[alt_names]
DNS.1 = localhost
IP.1 = 127.0.0.1
EOL

# Generate a new private key
openssl genrsa -out "${key_path}" 2048

# Create a certificate signing request (CSR) with additional SAN
openssl req -new -key "${key_path}" -out localhost.csr -config "${config_file}"

# Generate a self-signed certificate
openssl x509 -req -days 365 -in localhost.csr -signkey "${key_path}" -out "${cert_path}" -extensions v3_req -extfile "${config_file}"

# Clean up files
rm localhost.csr
rm "${config_file}"

echo "Private key stored at: ${key_path}"
echo "Certificate stored at: ${cert_path}"
