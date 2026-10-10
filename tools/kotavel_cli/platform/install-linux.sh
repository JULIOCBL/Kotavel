#!/bin/sh
# Preparación específica de Linux. Configuración recibida del perfil por install.py.
set -eu
if [ "$(uname -s)" != Linux ]; then
    echo 'Este instalador requiere Linux.' >&2
    exit 1
fi
exec sh "$(dirname "$0")/install-posix.sh" "$@"
