#!/bin/bash

set -e

BINARY="${1:-console}"

AVAILABLE_BINARIES=(
    "console"
    "backend"
    "frontend"
    "realtime"
    "worker"
    "rate-limiter"
)

show_usage() {
    echo "Usage: $0 [binary_name]"
    echo ""
    echo "Available binaries:"
    for binary in "${AVAILABLE_BINARIES[@]}"; do
        echo "  - $binary"
    done
    echo ""
    echo "Examples:"
    echo "  $0                # Run default console"
    echo "  $0 backend        # Run console with backend-api features"
    echo "  $0 frontend       # Run console with frontend-api features"
    echo "  $0 realtime       # Run realtime service"
}

is_valid_binary() {
    local binary="$1"
    for available in "${AVAILABLE_BINARIES[@]}"; do
        if [[ "$available" == "$binary" ]]; then
            return 0
        fi
    done
    return 1
}

if [[ "$1" == "-h" || "$1" == "--help" ]]; then
    show_usage
    exit 0
fi

if ! is_valid_binary "$BINARY"; then
    echo "Error: Invalid binary name '$BINARY'"
    echo ""
    show_usage
    exit 1
fi

BINARY_PATH="/app/$BINARY"
if [[ ! -f "$BINARY_PATH" ]]; then
    echo "Error: Binary '$BINARY' not found at $BINARY_PATH"
    exit 1
fi

if [[ ! -x "$BINARY_PATH" ]]; then
    echo "Error: Binary '$BINARY' is not executable"
    exit 1
fi

echo "Starting $BINARY..."
exec "$BINARY_PATH" "${@:2}"
