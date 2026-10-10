#!/bin/sh
# Preparación compartida por los sistemas POSIX; no instala paquetes con sudo.
set -eu
script_directory=${0%/*}
if [ "$script_directory" = "$0" ]; then script_directory=.; fi
exec "$1" "$script_directory/../posix_environment.py" "$2" "$3"
