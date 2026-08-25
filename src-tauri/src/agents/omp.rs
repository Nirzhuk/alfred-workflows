use super::pi::{run_pi_family, PiFamilyCli};
use super::{AgentAdapter, AgentError, AgentProvider, AgentRequest, AgentResponse, AgentRunHooks};

/// Adapter for OMP / oh-my-pi (`omp`).
///
/// OMP is a pi fork, so it shares pi's print mode and JSON event stream; only
/// the binary and the approval flag differ.
pub struct OmpAdapter;

const OMP: PiFamilyCli = PiFamilyCli {
    provider: AgentProvider::Omp,
    bin: "omp",
    missing_hint:
        "OMP CLI not found. Install `@oh-my-pi/pi-coding-agent` and ensure `omp` is on PATH.",
    // Unlike pi, OMP asks before each tool call; a workflow step has nobody to ask.
    unattended_args: &["--auto-approve"],
};

impl AgentAdapter for OmpAdapter {
    fn provider(&self) -> AgentProvider {
        AgentProvider::Omp
    }

    fn run(
        &self,
        request: AgentRequest,
        hooks: AgentRunHooks<'_>,
    ) -> Result<AgentResponse, AgentError> {
        run_pi_family(&OMP, request, hooks)
    }
}
