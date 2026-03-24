#!/usr/bin/env bash
# =============================================================================
# github-status.sh — Report commit statuses to GitHub
#
# Usage:  .ci/github-status.sh <state> <context> <description>
#
#   state:        pending | success | failure | error
#   context:      A short label like "build", "test", "lint", "deploy"
#   description:  Human-readable status message
#
# Environment variables (set automatically by GitHub Actions):
#   GITHUB_TOKEN       — Auth token with repo:status scope
#   GITHUB_REPOSITORY  — owner/repo
#   GITHUB_SHA         — Full commit SHA
#   GITHUB_SERVER_URL  — API base (default: https://github.com)
#   GITHUB_RUN_ID      — Current workflow run ID (used for target_url)
#
# Outside CI ($CI unset), this script is a silent no-op so developers can
# run `just build` locally without errors.
# =============================================================================

set -euo pipefail

STATE="${1:?Usage: github-status.sh <state> <context> <description>}"
CONTEXT="${2:?}"
DESCRIPTION="${3:-}"

# --- No-op outside CI --------------------------------------------------------
if [ -z "${CI:-}" ]; then
    exit 0
fi

# --- Validate ----------------------------------------------------------------
if [[ ! "$STATE" =~ ^(pending|success|failure|error)$ ]]; then
    echo "error: Invalid state '$STATE'. Must be: pending, success, failure, error" >&2
    exit 1
fi

# --- Required environment ----------------------------------------------------
: "${GITHUB_TOKEN:?GITHUB_TOKEN is required in CI}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required in CI}"
: "${GITHUB_SHA:?GITHUB_SHA is required in CI}"

API_BASE="${GITHUB_API_URL:-https://api.github.com}"
SERVER_URL="${GITHUB_SERVER_URL:-https://github.com}"
RUN_ID="${GITHUB_RUN_ID:-}"
TARGET_URL=""

if [ -n "$RUN_ID" ]; then
    TARGET_URL="${SERVER_URL}/${GITHUB_REPOSITORY}/actions/runs/${RUN_ID}"
fi

# --- Post status --------------------------------------------------------------
HTTP_CODE=$(curl -sf -o /dev/null -w "%{http_code}" \
    -X POST \
    -H "Authorization: token ${GITHUB_TOKEN}" \
    -H "Accept: application/vnd.github+json" \
    "${API_BASE}/repos/${GITHUB_REPOSITORY}/statuses/${GITHUB_SHA}" \
    -d "{
        \"state\": \"${STATE}\",
        \"context\": \"just/${CONTEXT}\",
        \"description\": \"${DESCRIPTION}\",
        \"target_url\": \"${TARGET_URL}\"
    }" 2>/dev/null) || true

if [ "${HTTP_CODE:-0}" -ge 200 ] && [ "${HTTP_CODE:-0}" -lt 300 ]; then
    echo "GitHub status: ${STATE} [just/${CONTEXT}] — ${DESCRIPTION}"
else
    echo "warning: Failed to post GitHub status (HTTP ${HTTP_CODE:-???})" >&2
    # Non-fatal — don't block the build over a status API failure
fi
