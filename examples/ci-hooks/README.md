# CI Hooks Example: Replacing GitHub Actions / Jenkinsfile with `just`

This example demonstrates how a **CI/CD engineer** can layer CI-specific
behavior (status reporting, artifact collection, environment setup) on top of a
**developer-authored** justfile — without touching the original recipes.

## Directory Layout

```
.
├── justfile              # Written by the app developer
├── .ci/
│   ├── hooks.just        # Written by the CI/CD engineer (imported by justfile)
│   └── github-status.sh  # Helper: GitHub commit status API calls
├── scripts/
│   └── deploy.sh         # Application deploy script
├── Jenkinsfile           # BEFORE (replaced by: just <recipe>)
└── .github/
    └── workflows/
        └── ci.yml        # BEFORE (replaced by: just <recipe>)
```

## How It Works

The app developer writes recipes for **what** the project needs:

```just
build:
    cargo build --release

test:
    cargo test

deploy target:
    ./scripts/deploy.sh {{target}}
```

The CI engineer writes hooks in `.ci/hooks.just` for **how CI wraps** those
recipes — status reporting, artifacts, notifications — and the developer adds
a single `import` line:

```just
import '.ci/hooks.just'
```

That one line is the only change required in the main justfile.

## What replaces what?

| GitHub Actions / Jenkins concept | `just` equivalent                          |
|----------------------------------|--------------------------------------------|
| `jobs.build`                     | `just build`                               |
| `jobs.test`                      | `just test`                                |
| `jobs.deploy`                    | `just deploy production`                   |
| `steps.actions/checkout`         | handled by CI runner (unchanged)           |
| `steps.setup-toolchain`          | `[before("build")]` hook                   |
| `steps.post-status`              | `[after("build")]` / `[after("test")]`     |
| `steps.upload-artifacts`         | `[after("test")]` hook                     |
| `steps.notify-slack`             | `[after("deploy")]` hook                   |
| `on.failure` / `post`            | exit-trap in hook scripts                  |

## Running Locally vs CI

Developers run the same recipes locally:

```sh
just build          # hooks run, but status scripts no-op outside CI
just test
just --no-hooks build   # skip hooks entirely for a fast local build
```

CI runs the exact same commands:

```sh
# In CI, environment variables like CI=true, GITHUB_TOKEN, etc. are set
just build
just test
just deploy production
```

The hook scripts detect CI via environment variables and only call external
APIs (GitHub status, Slack) when running in CI.

## Workflow: Who edits what?

- **App developer** owns `justfile` — adds/modifies recipes for build, test,
  deploy, lint, etc.
- **CI/CD engineer** owns `.ci/hooks.just` and `.ci/*.sh` — adds hooks that
  wrap recipes with CI plumbing. They never need to edit the main justfile.
- **Both** can work independently. Adding a new recipe doesn't require a hook.
  Adding a hook doesn't require changing the recipe.
