#!/bin/sh
# Preparación específica de macOS. Configuración recibida del perfil por install.py.
set -eu
if [ "$(uname -s)" != Darwin ]; then
    echo 'Este instalador requiere macOS.' >&2
    exit 1
fi
exec sh "$(dirname "$0")/install-posix.sh" "$@"
