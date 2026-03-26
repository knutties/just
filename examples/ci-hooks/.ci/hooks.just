# =============================================================================
# CI Hooks — owned by the CI/CD engineer
#
# These recipes hook into the developer's recipes via [before] and [after]
# attributes. They handle:
#   - GitHub commit status reporting (pending / success / failure)
#   - Artifact collection (test results, coverage, docs)
#   - Slack/Teams notifications on deploy
#   - CI environment setup
#
# Outside CI (when $CI is unset), the status-reporting scripts are no-ops,
# so developers can run `just build` locally without side effects.
# =============================================================================

# --- Build hooks -------------------------------------------------------------

# Report "pending" status to GitHub before the build starts
[before("build")]
[group("ci")]
ci-build-pending:
    #!/usr/bin/env bash
    .ci/github-status.sh pending build "Build is running..."

# Report "success" to GitHub after a successful build
[after("build")]
[group("ci")]
ci-build-success:
    #!/usr/bin/env bash
    .ci/github-status.sh success build "Build passed"

# --- Test hooks --------------------------------------------------------------

# Report "pending" status and prepare the test results directory
[before("test")]
[group("ci")]
ci-test-pending:
    #!/usr/bin/env bash
    .ci/github-status.sh pending test "Tests are running..."
    if [ -n "$CI" ]; then
        mkdir -p "${CI_ARTIFACTS_DIR:-./artifacts}/test-results"
        echo "::group::Test output"   # GitHub Actions log grouping
    fi

# Report test status and upload artifacts
[after("test")]
[group("ci")]
ci-test-success:
    #!/usr/bin/env bash
    .ci/github-status.sh success test "All tests passed"
    if [ -n "$CI" ]; then
        echo "::endgroup::"
        # Copy test results to the CI artifacts directory
        if [ -d target/test-results ]; then
            cp -r target/test-results "${CI_ARTIFACTS_DIR:-./artifacts}/"
            echo "Test artifacts copied to ${CI_ARTIFACTS_DIR:-./artifacts}"
        fi
        # Upload coverage if available
        if [ -f target/llvm-cov/lcov.info ]; then
            cp target/llvm-cov/lcov.info "${CI_ARTIFACTS_DIR:-./artifacts}/"
            echo "Coverage report uploaded"
        fi
    fi

# --- Lint hooks --------------------------------------------------------------

[before("lint")]
[group("ci")]
ci-lint-pending:
    #!/usr/bin/env bash
    .ci/github-status.sh pending lint "Lint checks running..."

[after("lint")]
[group("ci")]
ci-lint-success:
    #!/usr/bin/env bash
    .ci/github-status.sh success lint "Lint checks passed"

# --- Deploy hooks ------------------------------------------------------------

# Verify the CI environment is ready before deploying
[before("deploy")]
[group("ci")]
ci-deploy-gate:
    #!/usr/bin/env bash
    if [ -n "$CI" ]; then
        echo "--- Pre-deploy checks ---"
        # Ensure required secrets are available
        for var in DEPLOY_TOKEN DEPLOY_HOST SLACK_WEBHOOK_URL; do
            if [ -z "${!var}" ]; then
                echo "error: Required CI variable $var is not set"
                exit 1
            fi
        done
        .ci/github-status.sh pending deploy "Deploying..."
        echo "All pre-deploy checks passed"
    fi

# Notify Slack and update GitHub status after a successful deploy
[after("deploy")]
[group("ci")]
ci-deploy-notify:
    #!/usr/bin/env bash
    if [ -z "$CI" ]; then exit 0; fi

    SHA=$(git rev-parse --short HEAD)
    REPO="${GITHUB_REPOSITORY:-unknown}"
    RUN_URL="${GITHUB_SERVER_URL:-https://github.com}/${REPO}/actions/runs/${GITHUB_RUN_ID:-0}"

    .ci/github-status.sh success deploy "Deployed successfully"

    # Post to Slack
    if [ -n "$SLACK_WEBHOOK_URL" ]; then
        curl -sf -X POST "$SLACK_WEBHOOK_URL" \
            -H 'Content-Type: application/json' \
            -d "{
                \"text\": \":rocket: *${REPO}* deployed \`${SHA}\`\",
                \"blocks\": [
                    {
                        \"type\": \"section\",
                        \"text\": {
                            \"type\": \"mrkdwn\",
                            \"text\": \":rocket: *${REPO}* deployed \`${SHA}\`\n<${RUN_URL}|View CI run>\"
                        }
                    }
                ]
            }" && echo "Slack notified" || echo "warning: Slack notification failed"
    fi

# --- Docs hooks --------------------------------------------------------------

# Upload generated docs to GitHub Pages artifact
[after("docs")]
[group("ci")]
ci-docs-upload:
    #!/usr/bin/env bash
    if [ -z "$CI" ]; then exit 0; fi
    if [ -d target/doc ]; then
        cp -r target/doc "${CI_ARTIFACTS_DIR:-./artifacts}/docs"
        echo "Docs artifact uploaded"
    fi
