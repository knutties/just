use super::*;

/// Context passed to hook recipes via environment variables.
#[derive(Clone, Debug)]
pub(crate) struct HookContext {
  pub(crate) target: String,
  pub(crate) hook_type: HookType,
}

#[derive(Clone, Debug)]
pub(crate) enum HookType {
  Before,
  After { status: i32 },
}

impl HookContext {
  pub(crate) fn before(target: &str) -> Self {
    Self {
      target: target.to_string(),
      hook_type: HookType::Before,
    }
  }

  pub(crate) fn after(target: &str, status: i32) -> Self {
    Self {
      target: target.to_string(),
      hook_type: HookType::After { status },
    }
  }

  pub(crate) fn export(&self, cmd: &mut Command) {
    cmd.env("JUST_HOOK_TARGET", &self.target);

    match &self.hook_type {
      HookType::Before => {
        cmd.env("JUST_HOOK_TYPE", "before");
      }
      HookType::After { status } => {
        cmd.env("JUST_HOOK_TYPE", "after");
        cmd.env("JUST_HOOK_STATUS", status.to_string());
      }
    }
  }
}
