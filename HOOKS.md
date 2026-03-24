# Justfile Hooks

Hooks allow recipes to automatically run before or after a target recipe, without modifying the target recipe itself. This is especially useful for CI/CD pipelines (GitHub Actions, Jenkins, etc.) where setup and teardown logic needs to be injected around existing recipes.

## Quick Start

```just
[before("build")]
lint:
    cargo clippy --all-targets

[after("build")]
notify:
    echo "Build complete!"

build:
    cargo build --release
```

Running `just build` will execute in this order:

1. `lint` (before hook)
2. `build` (main recipe)
3. `notify` (after hook)

## Attributes

### `[before("target")]`

Marks a recipe to run **before** the specified target recipe.

```just
[before("deploy")]
check-env:
    test -n "$API_KEY" || (echo "API_KEY not set" && exit 1)

deploy:
    ./deploy.sh
```

### `[after("target")]`

Marks a recipe to run **after** the specified target recipe (only if the target succeeds).

```just
[after("test")]
coverage-report:
    cargo llvm-cov report

test:
    cargo test
```

## Multiple Hooks

Multiple hooks can target the same recipe. They execute in the order they appear in the justfile.

```just
[before("deploy")]
lint:
    cargo clippy

[before("deploy")]
test:
    cargo test

[after("deploy")]
notify-slack:
    curl -X POST "$SLACK_WEBHOOK" -d '{"text": "Deployed!"}'

[after("deploy")]
update-status:
    echo "deployed" > .deploy-status

deploy:
    ./deploy.sh
```

Execution order: `lint` -> `test` -> `deploy` -> `notify-slack` -> `update-status`

## A Single Hook for Multiple Targets

A recipe can have multiple `[before]` or `[after]` attributes to hook into several targets.

```just
[before("build")]
[before("test")]
[before("deploy")]
check-deps:
    cargo check
```

`check-deps` will run before `build`, `test`, and `deploy`.

## Hook Recipes Are Regular Recipes

Hook recipes can be invoked directly like any other recipe:

```just
[before("build")]
lint:
    cargo clippy

# Run lint directly without triggering build
# $ just lint
```

They can also have their own dependencies, attributes, and use any feature available to normal recipes:

```just
[before("deploy")]
[confirm("Run pre-deploy checks?")]
[group("ci")]
pre-deploy: lint test
    echo "All checks passed"
```

## Interaction with Dependencies

Hooks and dependencies work together. When running a recipe, the execution order is:

1. **Before hooks** of the target recipe
2. **Prior dependencies** (dependencies before `&&`)
3. **Recipe body**
4. **Subsequent dependencies** (dependencies after `&&`)
5. **After hooks** of the target recipe

```just
[before("build")]
setup:
    @echo 'hook: setup'

build: fetch-deps && cleanup
    @echo 'build'

fetch-deps:
    @echo 'dep: fetch-deps'

cleanup:
    @echo 'dep: cleanup'
```

Running `just build` outputs:

```
hook: setup
dep: fetch-deps
build
dep: cleanup
```

Hooks also run when a recipe is executed as a dependency of another recipe:

```just
[before("test")]
setup:
    @echo 'setup'

ci: test
    @echo 'ci done'

test:
    @echo 'test'
```

Running `just ci` outputs:

```
setup
test
ci done
```

## Skipping Hooks

Use the `--no-hooks` flag to skip all hook execution:

```sh
just --no-hooks deploy
```

This runs only `deploy` and its dependencies, without any before/after hooks.

The flag can also be set via the environment variable `JUST_NO_HOOKS=true`.

## Compile-Time Validation

Hook targets are validated at parse time. If a `[before]` or `[after]` attribute references a recipe that does not exist, `just` will report an error:

```
error: Hook recipe `setup` targets unknown recipe `nonexistent`
```

## Use Case: CI/CD Pipelines

Hooks are designed to make justfiles work well as CI drivers. A base justfile can define core recipes, and an imported CI-specific justfile can add hooks without modifying the originals:

**justfile:**
```just
import 'ci.just'

build:
    cargo build --release

test:
    cargo test

deploy:
    ./deploy.sh
```

**ci.just:**
```just
[before("build")]
ci-setup:
    echo "Setting up CI environment"
    apt-get install -y build-essential

[after("deploy")]
ci-notify:
    curl -X POST "$WEBHOOK_URL" -d '{"status": "deployed"}'

[after("test")]
ci-upload-results:
    cp target/test-results.xml $CI_ARTIFACTS_DIR/
```

This keeps the main justfile clean while allowing CI-specific behavior to be layered on top.
