use super::ExplorerView;
use gpui::Context;
use refscape_application::Command;
use std::time::Duration;

impl ExplorerView {
    pub(super) fn watch_sources(&mut self, cx: &mut Context<Self>) {
        // This task owns only a weak entity. Dropping the window cancels its timer;
        // file reads and analysis are effects, never work on the UI thread.
        self._reload_task = Some(cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if view
                    .update(cx, |view, cx| view.command(Command::RefreshSources, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
}
