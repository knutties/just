# Justfile Hooks

Hooks allow recipes to automatically run before or after a target recipe, without modifying the target recipe itself. This is especially useful for CI/CD pipelines (GitHub Actions, Jenkins, etc.) where setup and teardown logic needs to be injected around existing recipes.

## Table of Contents

- [Quick Start](#quick-start)
- [Attributes](#attributes)
- [Multiple Hooks](#multiple-hooks)
- [A Single Hook for Multiple Targets](#a-single-hook-for-multiple-targets)
- [Hook Recipes Are Regular Recipes](#hook-recipes-are-regular-recipes)
- [Interaction with Dependencies](#interaction-with-dependencies)
- [Skipping Hooks](#skipping-hooks)
- [Compile-Time Validation](#compile-time-validation)
- [Hook Context Variables](#hook-context-variables)
- [Use Case: CI/CD Pipelines](#use-case-cicd-pipelines)
- [Hooks as a CI/CD Replacement: Capability Assessment](#hooks-as-a-cicd-replacement-capability-assessment)
- [Comparison with Nix](#comparison-with-nix)
- [Summary](#summary)

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

## Hook Context Variables

When a hook recipe runs, `just` sets environment variables that provide information about the target recipe. This allows hooks to adapt their behavior based on which recipe triggered them and, for after hooks, whether the target succeeded.

### Variables Available to All Hooks

| Variable | Description | Example |
|---|---|---|
| `JUST_HOOK_TARGET` | Name of the target recipe that triggered the hook | `deploy` |
| `JUST_HOOK_TYPE` | Whether this is a `before` or `after` hook | `before` |

### Additional Variables for After Hooks

| Variable | Description | Example |
|---|---|---|
| `JUST_HOOK_STATUS` | Exit status of the target recipe (`0` for success) | `0` |

### Why This Matters

A single hook recipe can be attached to multiple targets:

```just
[before("build")]
[before("test")]
[before("deploy")]
check:
    echo "Running checks before $JUST_HOOK_TARGET"
```

Without `JUST_HOOK_TARGET`, the hook has no way to know which recipe triggered it. This is essential for logging, conditional logic, and status reporting.

### Example: Status Reporting

```just
[after("build")]
[after("test")]
[after("deploy")]
report-status:
    curl -X POST "$WEBHOOK_URL" \
      -d "{\"recipe\": \"$JUST_HOOK_TARGET\", \"status\": \"$JUST_HOOK_STATUS\"}"
```

### Direct Invocation

When a hook recipe is run directly (e.g., `just check`), hook context variables are **not** set. The recipe runs as a normal recipe without any hook-specific environment. This allows hooks to detect whether they were triggered as a hook or invoked directly:

```just
[before("build")]
setup:
    if [ -n "$JUST_HOOK_TARGET" ]; then
        echo "Running as hook for $JUST_HOOK_TARGET"
    else
        echo "Running directly"
    fi
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

## Hooks as a CI/CD Replacement: Capability Assessment

Hooks combined with existing `just` features can replace a significant portion of what CI/CD workflow files (e.g., GitHub Actions YAML) provide. This section assesses which CI capabilities hooks can cover today, which could be covered with configurable hook settings, and which fundamentally require an external platform.

### Already Supported by Hooks + Just

| Capability | How |
|---|---|
| **Step ordering** | `[before]` / `[after]` hooks around recipes |
| **Setup / teardown** | Before hooks for setup, after hooks for cleanup |
| **Conditional execution** | Shell conditionals in hooks (e.g., `test -n "$CI"`) |
| **Notifications / status reporting** | After hooks calling webhooks, APIs, etc. |
| **Artifact collection** | After hooks copying files to `$CI_ARTIFACTS_DIR` |
| **Job-level parallelism** | `just`'s native recipe dependencies with `--parallel` — hooks fire per-recipe, so each parallel branch gets its own before/after hooks |
| **DAG-like dependency graphs** | `just`'s dependency system already models this; hooks layer on top |
| **Separation of concerns** | `import` to layer CI hooks over a clean base justfile |

### Addressable with Configurable Hook Settings

These capabilities don't exist today but could be added as declarative settings on hooks without requiring an external CI platform:

| Capability | Possible Setting | How It Would Work |
|---|---|---|
| **Matrix builds** | `matrix = { os = ["linux", "macos"], rust = ["stable", "nightly"] }` | Hook runner expands into multiple invocations with env vars like `$HOOK_MATRIX_OS`, `$HOOK_MATRIX_RUST` |
| **Simple triggers** | `triggers = ["file-change", "schedule"]` | Local daemon, file watcher, or cron invokes recipes. A `schedule = "0 */6 * * *"` setting is just cron |
| **Secrets injection** | `secrets = ["AWS_KEY", "DB_PASS"]` | Hook runner pulls from a local secrets store (e.g., `pass`, 1Password CLI, HashiCorp Vault) and injects into the environment |
| **Service containers** | `services = { postgres = { image = "postgres:15", port = 5432 } }` | Hook runner starts/stops Docker sidecars around the recipe |
| **Declarative conditions** | `when = "branch == main"` or `when = "$CI == true"` | Makes hook activation declarative instead of requiring shell `if` blocks |
| **Runner specification** | `runner = { os = "linux", arch = "x86_64", tools = ["rust:1.75", "node:20"] }` | Declares the required environment — locally, validates or uses `docker`/`nix-shell`/`devcontainer` to satisfy it; in CI, the platform maps it to its runner pool. Follows the same convention-based approach as `$CI=true`, `.node-version`, `.tool-versions` (asdf), and `rust-toolchain.toml` |

### Remaining Gaps: Requires an External Platform

These capabilities are inherently platform-level concerns that cannot be replicated by a build tool, no matter how configurable:

| Capability | Why It Needs a Platform |
|---|---|
| **Runner provisioning** | Someone must **be** the machine. Declaring `runner: { os: macos, arch: arm64 }` is standardizable, but actually provisioning that machine requires an orchestrator with a pool of runners |
| **Platform identity (OIDC / token scoping)** | Keyless cloud authentication (e.g., `permissions: id-token: write` for AWS/GCP federation) requires the CI platform to **be** the identity provider. No local config can replicate this |
| **Cross-run persistence** | Caching and artifact storage across ephemeral CI runs (e.g., `actions/cache`, `actions/upload-artifact`) require a persistence layer that outlives any single build. Locally your filesystem already serves this purpose, but ephemeral CI runners need platform-managed storage |

## Comparison with Nix

Justfile hooks and Nix solve different problems with fundamentally different philosophies. Understanding where each excels helps clarify why they are complementary rather than competitive.

### Philosophy

| | Justfile Hooks | Nix |
|---|---|---|
| **Core model** | Imperative shell commands with declarative ordering | Purely functional derivations — inputs → outputs |
| **Hermeticity** | None — recipes see the full host environment | Strict — builds run in sandboxed environments with only declared inputs |
| **Reproducibility** | Best-effort (depends on what's installed) | Guaranteed by design (content-addressed store) |
| **Caching** | None built-in (your filesystem is the cache) | Automatic — if inputs haven't changed, the output is already in `/nix/store` |
| **Composability** | `import` + `[before]`/`[after]` hooks layer behavior | Overlays, overrides, and flake inputs compose packages |

### Strengths of Each Approach

**Justfile hooks are better when you want:**
- Low barrier to entry — a `justfile` is just shell commands with names
- Transparent execution — you can read and predict exactly what runs
- Incremental adoption — drop a `ci.just` into any project, no ecosystem buy-in
- Human-oriented workflows — confirmations, notifications, status reporting
- CI/CD glue — hooks model the "around the build" concerns (setup, teardown, notifications) that Nix doesn't address

**Nix is better when you want:**
- Reproducible environments — every developer and CI runner gets exactly the same toolchain
- Hermetic builds — no "works on my machine" failures
- Cross-project dependency management — Nix resolves the entire dependency graph (system libs, toolchains, services)
- Content-addressed caching — rebuilds only what actually changed, across machines
- Declarative environments — `nix develop` gives you a shell with all tools, no manual install steps

### How They Express Build Ordering

Both can express "run lint before build, run tests after build," but differently:

- **Nix** does this via derivation dependencies in a DAG. The ordering is a *consequence* of data flow (build needs lint's output). It's implicit and enforced by the build graph.
- **Justfile hooks** do this via explicit `[before]`/`[after]` annotations. The ordering is a *declaration* by the author. It's explicit and enforced by the runner.

### Where They Don't Compete

| Concern | Justfile Hooks | Nix |
|---|---|---|
| **Environment setup** | Shell commands (`apt install`, `brew install`) — fragile | `nix develop` — hermetic and reproducible |
| **Build orchestration** | First-class (`[before]`/`[after]`, `--parallel`) | First-class (derivation DAG) |
| **Notifications / webhooks** | Natural (after hooks calling `curl`) | Unnatural (Nix is about building, not side effects) |
| **Secret injection** | Possible (env vars, proposed `secrets` setting) | Actively hostile (secrets break reproducibility) |
| **CI layering** | `import 'ci.just'` — clean separation | Flake outputs per system — but CI logic lives elsewhere |

### Using Them Together

A realistic combined setup uses Nix for the hermetic environment and build, while hooks provide the orchestration, side effects, and CI glue that Nix intentionally doesn't handle:

```just
[before("build")]
setup:
    nix develop --command echo "Environment ready"

build:
    nix build .#default

[after("build")]
notify:
    curl -X POST "$SLACK_WEBHOOK" -d '{"text": "Build complete"}'
```

### Key Tradeoff

**Nix** trades simplicity for correctness — you get guarantees, but the learning curve is steep and the ecosystem is opinionated. **Justfile hooks** trade correctness for simplicity — you get something working in minutes, but you're responsible for ensuring your environment is consistent.

The "Remaining Gaps" identified above (runner provisioning, identity, persistence) apply equally to both — neither Nix nor justfile hooks can *be* the CI platform. But Nix closes the "reproducible environment" gap that hooks leave open, while hooks close the "workflow orchestration" gap that Nix leaves open.

## Summary

The vast majority of CI/CD workflow logic — step orchestration, parallelism, dependency graphs, setup/teardown, notifications, and conditional execution — is already expressible through hooks and native `just` features. With configurable hook settings (matrix expansion, triggers, secrets, services, runner specs), the surface area that **must** live in platform-specific workflow files shrinks to just three concerns: runner provisioning, platform identity, and cross-run persistence.

Hooks and Nix are complementary rather than competitive. Nix provides hermetic, reproducible environments and builds; hooks provide workflow orchestration, side effects, and CI glue. Used together, they cover nearly the full spectrum of CI/CD needs, with only runner provisioning, platform identity, and cross-run persistence requiring an external platform.
