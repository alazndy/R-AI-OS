//! The status-bar quota signal: `raios usage` read on the background executor.

use std::sync::Arc;
use std::time::Duration;

use gpui::Context;
use rwk_core::usage::UsageSource;

use super::Workbench;

/// How often the quota signal is re-read while the window lives. The figures move on a
/// five-hour window, so a slow poll is enough — and every reading stays off the UI thread.
pub const USAGE_REFRESH: Duration = Duration::from_secs(600);

impl Workbench {
    /// Connects the quota source (the RAIOS CLI by default) and starts reading it.
    pub fn set_usage_source(
        &mut self,
        source: Option<Arc<dyn UsageSource>>,
        cx: &mut Context<Self>,
    ) {
        self.usage_source = source.clone();
        if source.is_none() {
            // Dropping the task cancels the loop.
            self.usage_task = None;
            return;
        }
        if self.usage_task.is_some() {
            return;
        }
        self.usage_task = Some(cx.spawn(async move |this, cx| loop {
            let Some(source) = this
                .update(cx, |workbench, _| workbench.usage_source.clone())
                .ok()
                .flatten()
            else {
                let _ = this.update(cx, |workbench, _| workbench.usage_task = None);
                return;
            };
            let report = cx
                .background_executor()
                .spawn(async move { source.usage() })
                .await;
            let _ = this.update(cx, |workbench, cx| {
                // A failed reading leaves the status bar quiet instead of toasting; the next
                // refresh tries again.
                workbench.quota = report.ok().and_then(|report| report.quota_line());
                cx.notify();
            });
            cx.background_executor().timer(USAGE_REFRESH).await;
        }));
    }

    /// The quota line the status bar shows, when a reading has landed.
    pub fn quota_line(&self) -> Option<String> {
        self.quota.clone()
    }
}
