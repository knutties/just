# Live Visualization Demo
#
# Run with: just --live -f examples/live-demo.just pipeline
#
# This demonstrates parallel and sequential recipe execution
# visualized in the browser via the --live flag.

# Lint the source code
lint:
    @echo "Linting..."
    @sleep 1
    @echo "No lint errors found."

# Check formatting
fmt-check:
    @echo "Checking formatting..."
    @sleep 0.8
    @echo "All files formatted correctly."

# Compile the project
[doc("Compile the project from source")]
build: lint
    @echo "Compiling src/main.rs..."
    @sleep 1.5
    @echo "Compiling src/lib.rs..."
    @sleep 1
    @echo "Build succeeded. 2 targets built."

# Run unit tests in parallel with integration tests
[parallel]
test-all: unit-tests integration-tests

# Run unit tests
unit-tests: build
    @echo "Running unit tests..."
    @sleep 1.2
    @echo "  test parse::basic ... ok"
    @sleep 0.3
    @echo "  test parse::nested ... ok"
    @sleep 0.3
    @echo "  test eval::simple ... ok"
    @echo "3 tests passed."

# Run integration tests
integration-tests: build
    @echo "Running integration tests..."
    @sleep 1.5
    @echo "  test cli::help ... ok"
    @sleep 0.5
    @echo "  test cli::run ... ok"
    @echo "2 tests passed."

# Package the release artifact
package: test-all
    @echo "Creating release archive..."
    @sleep 1
    @echo "Wrote release/app-1.0.0.tar.gz (2.4 MB)"

# Deploy to staging
deploy-staging: package
    @echo "Uploading to staging..."
    @sleep 1.5
    @echo "Deployed to staging.example.com"

# Full pipeline: lint, build, test, package, deploy
pipeline: deploy-staging fmt-check
    @echo ""
    @echo "=== Pipeline complete ==="
