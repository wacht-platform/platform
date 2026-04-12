#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEFAULT_MAC_ENV="$ROOT_DIR/.runtime/code_runner/venv"
DEFAULT_LINUX_ENV="/opt/wacht/code_runner/venv"

if [[ "${OSTYPE:-}" == darwin* ]]; then
  ENV_DIR="${CODE_RUNNER_ENV_DIR:-$DEFAULT_MAC_ENV}"
else
  ENV_DIR="${CODE_RUNNER_ENV_DIR:-$DEFAULT_LINUX_ENV}"
fi

REQ_FILE="${CODE_RUNNER_REQUIREMENTS_FILE:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/requirements-code-runner.txt}"
PYTHON_BIN="${CODE_RUNNER_BOOTSTRAP_PYTHON:-python3}"

echo "Using CodeRunner env: $ENV_DIR"
echo "Using requirements: $REQ_FILE"

mkdir -p "$(dirname "$ENV_DIR")"

if [[ ! -x "$ENV_DIR/bin/python" ]]; then
  "$PYTHON_BIN" -m venv "$ENV_DIR"
fi

"$ENV_DIR/bin/python" -m pip install --upgrade pip
"$ENV_DIR/bin/pip" install -r "$REQ_FILE"

echo
echo "CodeRunner environment ready."
echo "Python: $ENV_DIR/bin/python"
echo "Set CODE_RUNNER_PYTHON_PATH=$ENV_DIR/bin/python on worker hosts."
