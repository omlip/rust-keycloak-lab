FROM debian:trixie-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    samba-ad-dc samba-ad-provision python3-samba python3-ldb \
    openssl ldap-utils ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY entrypoint.py /opt/ad/entrypoint.py
COPY healthcheck.py /opt/ad/healthcheck.py
ENTRYPOINT ["python3", "/opt/ad/entrypoint.py"]
