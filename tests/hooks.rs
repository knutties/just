use super::*;

#[test]
fn before_hook_runs_before_recipe() {
  Test::new()
    .justfile(
      "
        [before(\"foo\")]
        setup:
          @echo 'before'

        foo:
          @echo 'main'
      ",
    )
    .args(["foo"])
    .stdout("before\nmain\n")
    .success();
}

#[test]
fn after_hook_runs_after_recipe() {
  Test::new()
    .justfile(
      "
        [after(\"foo\")]
        cleanup:
          @echo 'after'

        foo:
          @echo 'main'
      ",
    )
    .args(["foo"])
    .stdout("main\nafter\n")
    .success();
}

#[test]
fn before_and_after_hooks() {
  Test::new()
    .justfile(
      "
        [before(\"foo\")]
        setup:
          @echo 'before'

        [after(\"foo\")]
        cleanup:
          @echo 'after'

        foo:
          @echo 'main'
      ",
    )
    .args(["foo"])
    .stdout("before\nmain\nafter\n")
    .success();
}

#[test]
fn no_hooks_flag_skips_hooks() {
  Test::new()
    .justfile(
      "
        [before(\"foo\")]
        setup:
          @echo 'before'

        [after(\"foo\")]
        cleanup:
          @echo 'after'

        foo:
          @echo 'main'
      ",
    )
    .args(["--no-hooks", "foo"])
    .stdout("main\n")
    .success();
}

#[test]
fn multiple_before_hooks() {
  Test::new()
    .justfile(
      "
        [before(\"foo\")]
        setup-a:
          @echo 'before-a'

        [before(\"foo\")]
        setup-b:
          @echo 'before-b'

        foo:
          @echo 'main'
      ",
    )
    .args(["foo"])
    .stdout("before-a\nbefore-b\nmain\n")
    .success();
}

#[test]
fn hooks_with_dependencies() {
  Test::new()
    .justfile(
      "
        [before(\"foo\")]
        setup:
          @echo 'before'

        foo: bar
          @echo 'main'

        bar:
          @echo 'bar'
      ",
    )
    .args(["foo"])
    .stdout("before\nbar\nmain\n")
    .success();
}

#[test]
fn hook_targets_unknown_recipe() {
  Test::new()
    .justfile(
      "
        [before(\"nonexistent\")]
        setup:
          @echo 'before'
      ",
    )
    .args(["setup"])
    .stderr_regex(".*Hook recipe `setup` targets unknown recipe `nonexistent`.*")
    .status(1);
}

#[test]
fn hook_recipe_can_be_run_directly() {
  Test::new()
    .justfile(
      "
        [before(\"foo\")]
        setup:
          @echo 'setup'

        foo:
          @echo 'main'
      ",
    )
    .args(["setup"])
    .stdout("setup\n")
    .success();
}

#[test]
fn hooks_do_not_run_on_dependency_recipes() {
  Test::new()
    .justfile(
      "
        [before(\"bar\")]
        setup:
          @echo 'before-bar'

        foo: bar
          @echo 'foo'

        bar:
          @echo 'bar'
      ",
    )
    .args(["foo"])
    .stdout("before-bar\nbar\nfoo\n")
    .success();
}
