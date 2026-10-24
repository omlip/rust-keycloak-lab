#!/bin/sh
set -eu
umask 077
install -d -o monitor -g monitor -m 0700 /run/cost-monitor
install -o monitor -g monitor -m 0600 "$LOGIN_CLIENT_SECRET_FILE" /run/cost-monitor/login.secret
install -o monitor -g monitor -m 0600 "$CATALOG_CLIENT_SECRET_FILE" /run/cost-monitor/catalog.secret
export LOGIN_CLIENT_SECRET_FILE=/run/cost-monitor/login.secret
export CATALOG_CLIENT_SECRET_FILE=/run/cost-monitor/catalog.secret
exec gosu monitor cost-monitor
