pub mod job_object;
pub mod manager;
pub mod scrollback;

pub use job_object::ProcessJobObject;
pub use manager::{ExitCallback, OutputCallback, PtyManager};
pub use scrollback::RingScrollback;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_exports_are_accessible() {
        // Verify all public types are re-exported correctly
        let _job: Option<ProcessJobObject> = None;
        let _scrollback: Option<RingScrollback> = None;
        let _manager: Option<PtyManager> = None;
        let _output_cb: Option<OutputCallback> = None;
        let _exit_cb: Option<ExitCallback> = None;
    }
}
